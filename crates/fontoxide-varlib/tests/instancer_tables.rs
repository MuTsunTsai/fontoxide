//! The tests of the variation tables of fontTools' `Tests/varLib/instancer/instancer_test.py`:
//! `CFF2`, `gvar`, `cvar`, `MVAR`, `HVAR`, `VVAR`, item variation stores, `avar` version 1,
//! `fvar` and `STAT`.

#![allow(clippy::type_complexity, clippy::field_reassign_with_default)]

mod common;
mod instancer_common;

use common::*;
use fontoxide_ttlib::Tag;
use fontoxide_ttlib::tables::Avar;
use fontoxide_ttlib::tables::Fvar;
use fontoxide_ttlib::tables::fvar::Axis;

use instancer_common::*;

fn tag(text: &str) -> Tag {
	Tag::from_str_padded(text).expect("a tag")
}

mod instantiate_cff2_test {
	use fontoxide_pens::{Pen, Point, Transform};
	use fontoxide_ttlib::GlyphSet;
	use fontoxide_ttlib::cff::charstring::Token;
	use fontoxide_ttlib::cff::dict::Value;
	use fontoxide_ttlib::cff::font::{FontDict, PrivateDict, PrivateRef, TopDict, num_regions};
	use fontoxide_ttlib::cff::specializer::{
		Arg, Command, commands_to_program, program_to_commands,
	};
	use fontoxide_ttlib::tables::Cff2;
	use fontoxide_ttlib::var::{Location, VarStoreInstancer};
	use fontoxide_ttlib::{Font, WriteOptions};
	use fontoxide_varlib::instancer::{NormalizedLimits, instantiate_cff2, instantiate_hvar};

	use super::*;

	fn instantiate(font: &mut Font, limits: &NormalizedLimits) -> TestResult {
		instantiate_cff2(font, limits, true, false, None)?;
		Ok(())
	}

	fn top(font: &mut Font) -> TestResult<&mut TopDict> {
		Ok(font
			.require::<Cff2>()?
			.cff
			.top_dicts
			.first_mut()
			.ok_or("no Top DICT")?)
	}

	fn program_text(program: &[Token]) -> String {
		let tokens: Vec<String> = program
			.iter()
			.map(|t| match t {
				Token::Int(v) => v.to_string(),
				Token::Float(v) => v.to_string(),
				Token::Op(op) => (*op).to_owned(),
				Token::Mask(_) => "mask".to_owned(),
			})
			.collect();
		tokens.join(" ")
	}

	#[test]
	fn test_pin_and_drop_axis() -> TestResult {
		let cases: [(&[(&str, &[f64])], &str); 4] = [
			(
				&[],
				"44 256 6 -29 2 blend rmoveto 239 35 -239 44 90 -44 3 blend hlineto",
			),
			(&[("wght", &[0.0])], "44 256 rmoveto 239 35 -239 hlineto"),
			(&[("wght", &[0.5])], "47 242 rmoveto 261 80 -261 hlineto"),
			(&[("wght", &[1.0])], "50 227 rmoveto 283 125 -283 hlineto"),
		];
		for (location, expected) in cases {
			let mut font = font_from_ttx(&instancer_data("CFF2Instancer-VF-1.ttx"))?;
			let location = normalized(location);
			instantiate(&mut font, &location)?;
			instantiate_hvar(&mut font, &location)?;
			let program = &top(&mut font)?
				.char_strings
				.as_ref()
				.ok_or("no CharStrings")?
				.items[1]
				.program;
			assert_eq!(program_text(program), expected);
		}
		Ok(())
	}

	#[test]
	fn test_no_varstore() -> TestResult {
		let mut font = font_from_ttx(&instancer_data("CFF2Instancer-VF-1.ttx"))?;
		// Fully instancing empties the VarStore and removes it from the table.
		let limits = normalized(&[("wght", &[0.0])]);
		instantiate(&mut font, &limits)?;
		assert!(top(&mut font)?.var_store.is_none());
		// A CFF2 table without a VariationStore must survive instancing.
		// https://github.com/fonttools/fonttools/issues/4130
		instantiate(&mut font, &limits)?;
		Ok(())
	}

	#[test]
	fn test_private_dict_vsindex() -> TestResult {
		// https://github.com/fonttools/fonttools/issues/4129
		let mut font = font_from_ttx(&instancer_data("CFF2Instancer-VF-2.ttx"))?;
		let axes = axis_tags(&mut font)?;
		let blue_values: [[i64; 4]; 2] = [[-12, 10, 20, 40], [0, 5, -5, 15]];
		// Add a Private dict with a non-zero vsindex and blended values; its blends
		// reference VarData 1, not VarData 0.
		let mut private = PrivateDict {
			cff2: true,
			..Default::default()
		};
		private.values.insert("vsindex", Value::Int(1));
		private.values.insert(
			"BlueValues",
			Value::List(
				blue_values
					.iter()
					.map(|v| Value::List(v.iter().map(|v| Value::Int(*v)).collect()))
					.collect(),
			),
		);
		let top_dict = top(&mut font)?;
		let fd_array = top_dict.fd_array.as_mut().ok_or("no FDArray")?;
		let index = fd_array.len();
		fd_array.push(FontDict {
			cff2: true,
			private: Some(Box::new(private)),
			..Default::default()
		});

		let pinned: Location = [("wght".to_owned(), 1.0), ("wdth".to_owned(), 1.0)]
			.into_iter()
			.collect();
		let mut expected = Vec::new();
		{
			let vsi = VarStoreInstancer::new(top_dict.var_store.as_deref(), &axes, pinned);
			let mut delta = 0.0;
			for value in blue_values {
				// Defaults are absolute; the encoded region deltas are relative.
				let deltas: Vec<f64> = value[1..].iter().map(|v| *v as f64).collect();
				delta += vsi.interpolate_from_deltas(1, &deltas)?;
				expected.push(value[0] as f64 + py_round(delta));
			}
		}

		instantiate(
			&mut font,
			&normalized(&[("wght", &[1.0]), ("wdth", &[1.0])]),
		)?;

		let top_dict = top(&mut font)?;
		let private = top_dict.fd_array.as_ref().ok_or("no FDArray")?[index]
			.private
			.as_ref()
			.ok_or("no Private")?;
		let Some(Value::List(values)) = private.values.get("BlueValues") else {
			return Err("no BlueValues".into());
		};
		let values: Vec<f64> = values.iter().filter_map(Value::number).collect();
		assert_eq!(values, expected);
		// The store emptied; the dict's vsindex must be gone with it.
		assert!(!private.values.contains_key("vsindex"));
		Ok(())
	}

	#[test]
	fn test_charstring_inherits_private_dict_vsindex() -> TestResult {
		let mut font = font_from_ttx(&instancer_data("CFF2Instancer-VF-2.ttx"))?;
		let command = || {
			Command::new(
				"rmoveto",
				vec![
					Arg::Int(50),
					Arg::Blend(vec![
						Arg::Int(100),
						Arg::Int(1),
						Arg::Int(2),
						Arg::Int(3),
						Arg::Int(1),
					]),
				],
			)
		};
		font.require::<Cff2>()?.cff.decompile_all_char_strings()?;
		let top_dict = top(&mut font)?;
		let char_strings = top_dict.char_strings.as_mut().ok_or("no CharStrings")?;
		let index = char_strings.index("A").ok_or("no glyph A")?;
		let private = char_strings.privates[index];
		// VarData 1 has three regions, unlike VarData 0, and is selected by the Private
		// DICT without an explicit vsindex operator in the charstring.
		let char_string = &mut char_strings.items[index];
		char_string.bytecode = None;
		char_string.program = commands_to_program(&[command()])?;
		let PrivateRef::Fd(fd) = private else {
			return Err("no font DICT".into());
		};
		let private_dict = top_dict.fd_array.as_mut().ok_or("no FDArray")?[fd]
			.private
			.as_mut()
			.ok_or("no Private")?;
		private_dict.values.insert("vsindex", Value::Int(1));

		instantiate(&mut font, &normalized(&[]))?;

		let top_dict = top(&mut font)?;
		let private_dict = top_dict.fd_array.as_ref().ok_or("no FDArray")?[fd]
			.private
			.as_ref()
			.ok_or("no Private")?;
		let vs_index = private_dict.get("vsindex").map_or(Ok(0), |v| v.int())?;
		let store = top_dict.var_store.as_deref();
		let get_num_regions = |vs: i64| num_regions(store, vs);
		let char_strings = top_dict.char_strings.as_ref().ok_or("no CharStrings")?;
		let commands = program_to_commands(
			&char_strings.items[index].program,
			Some(&get_num_regions),
			vs_index,
		)?;
		assert_eq!(commands, [command()]);
		Ok(())
	}

	#[derive(Default)]
	struct RecordingPen {
		value: Vec<&'static str>,
	}

	impl Pen for RecordingPen {
		fn move_to(&mut self, _: Point) -> fontoxide_pens::Result<()> {
			self.value.push("moveTo");
			Ok(())
		}

		fn line_to(&mut self, _: Point) -> fontoxide_pens::Result<()> {
			self.value.push("lineTo");
			Ok(())
		}

		fn curve_to(&mut self, _: &[Point]) -> fontoxide_pens::Result<()> {
			self.value.push("curveTo");
			Ok(())
		}

		fn q_curve_to(&mut self, _: &[Point], _: Option<Point>) -> fontoxide_pens::Result<()> {
			self.value.push("qCurveTo");
			Ok(())
		}

		fn close_path(&mut self) -> fontoxide_pens::Result<()> {
			self.value.push("closePath");
			Ok(())
		}

		fn end_path(&mut self) -> fontoxide_pens::Result<()> {
			self.value.push("endPath");
			Ok(())
		}

		fn add_component(&mut self, _: &str, _: Transform) -> fontoxide_pens::Result<()> {
			self.value.push("addComponent");
			Ok(())
		}
	}

	#[test]
	fn test_instance_of_saved_font() -> TestResult {
		// Saving before instancing used to leave a compiled VarStore behind that the second
		// save then wrote out in place of the pruned one.
		let mut font = font_from_ttx(&instancer_data("CFF2Instancer-VF-2.ttx"))?;
		font.write(&WriteOptions::default())?;

		common::instantiate(&mut font, &[("wdth", pin(100.0))])?;

		let mut instance = reload_default(&mut font)?;
		let axes = axis_tags(&mut instance)?;
		let top_dict = top(&mut instance)?;
		let store = top_dict.var_store.as_ref().ok_or("no VarStore")?;
		let region_list = store.var_region_list.as_ref().ok_or("no VarRegionList")?;
		assert_eq!(region_list.region_axis_count, Some(1));
		assert_eq!(axes.len(), 1);

		let mut pen = RecordingPen::default();
		GlyphSet::new(&mut instance)?.draw("A", &mut pen)?;
		assert!(!pen.value.is_empty());
		Ok(())
	}

	#[test]
	fn test_full_instance() -> TestResult {
		for (source_ttx, expected_ttx) in [
			(
				"CFF2Instancer-VF-1.ttx",
				"CFF2Instancer-VF-1-instance-400.ttx",
			),
			(
				"CFF2Instancer-VF-2.ttx",
				"CFF2Instancer-VF-2-instance-400.ttx",
			),
			(
				"CFF2Instancer-VF-3.ttx",
				"CFF2Instancer-VF-3-instance-400.ttx",
			),
		] {
			let mut font = font_from_ttx(&instancer_data(source_ttx))?;
			let mut font = reload_default(&mut font)?;

			common::instantiate(&mut font, &[("wght", pin(400.0))])?;
			let mut instance = reload_default(&mut font)?;
			let actual = stripped_ttx(&mut instance)?;

			let mut expected =
				font_from_ttx(&instancer_data(&format!("test_results/{expected_ttx}")))?;
			let mut expected = reload_default(&mut expected)?;
			let expected = stripped_ttx(&mut expected)?;

			assert_same_ttx(&actual, &expected, source_ttx);
		}
		Ok(())
	}
}

mod instantiate_gvar_test {
	use fontoxide_ttlib::Font;
	use fontoxide_ttlib::tables::{Gvar, Hmtx, Vmtx};
	use fontoxide_varlib::instancer::instantiate_gvar;

	use super::*;

	type Coords = [(f64, f64); 8];

	const WDTH_M1: Coords = [
		(27.0, 229.0),
		(27.0, 310.0),
		(247.0, 310.0),
		(247.0, 229.0),
		(0.0, 0.0),
		(274.0, 0.0),
		(0.0, 536.0),
		(0.0, 0.0),
	];
	const WDTH_M05: Coords = [
		(33.5, 229.0),
		(33.5, 308.5),
		(264.5, 308.5),
		(264.5, 229.0),
		(0.0, 0.0),
		(298.0, 0.0),
		(0.0, 536.0),
		(0.0, 0.0),
	];
	const DEFAULT: Coords = [
		(40.0, 229.0),
		(40.0, 307.0),
		(282.0, 307.0),
		(282.0, 229.0),
		(0.0, 0.0),
		(322.0, 0.0),
		(0.0, 536.0),
		(0.0, 0.0),
	];

	#[test]
	fn test_pin_and_drop_axis() -> TestResult {
		for (location, expected) in [(-1.0, &WDTH_M1), (-0.5, &WDTH_M05), (0.0, &DEFAULT)] {
			for optimize in [true, false] {
				let mut font = varfont()?;
				let location = normalized(&[("wdth", &[location])]);

				instantiate_gvar(&mut font, &location, optimize)?;

				assert_eq!(get_coordinates(&mut font, "hyphen")?, *expected);
				// check that the pinned axis has been dropped from gvar
				let gvar = font.require::<Gvar>()?;
				assert!(
					!gvar
						.variations
						.iter()
						.flat_map(|(_, tuples)| tuples)
						.any(|t| t.axes.contains_key("wdth"))
				);
			}
		}
		Ok(())
	}

	#[test]
	fn test_full_instance() -> TestResult {
		for optimize in [true, false] {
			let mut font = varfont()?;
			let location = normalized(&[("wght", &[0.0]), ("wdth", &[-0.5])]);

			instantiate_gvar(&mut font, &location, optimize)?;

			assert_eq!(get_coordinates(&mut font, "hyphen")?, WDTH_M05);
			assert!(!font.contains(Tag::new(b"gvar")));
		}
		Ok(())
	}

	fn metrics(font: &mut Font, name: &str) -> TestResult<((u16, i16), (u16, i16))> {
		let gid = font.glyph_id(name)?;
		let h = font.require::<Hmtx>()?.get(gid).ok_or("no hmtx")?;
		let v = font.require::<Vmtx>()?.get(gid).ok_or("no vmtx")?;
		Ok((h, v))
	}

	/// The 'minus' glyph is a composite glyph, which references 'hyphen' as a component, but
	/// has no tuple variations in gvar table, so the component offset and the phantom points
	/// do not change; however the sidebearings and bounding box do change as a result of the
	/// parent glyph 'hyphen' changing.
	#[test]
	fn test_composite_glyph_not_in_gvar() -> TestResult {
		let mut font = varfont()?;
		assert_eq!(get_coordinates(&mut font, "hyphen")?, DEFAULT);
		assert_eq!(metrics(&mut font, "hyphen")?, ((322, 40), (536, 229)));

		let minus_coords = get_coordinates(&mut font, "minus")?;
		assert_eq!(
			minus_coords,
			[
				(0.0, 0.0),
				(0.0, 0.0),
				(422.0, 0.0),
				(0.0, 536.0),
				(0.0, 0.0)
			]
		);
		assert_eq!(metrics(&mut font, "minus")?, ((422, 40), (536, 229)));

		let location = normalized(&[("wght", &[-1.0]), ("wdth", &[-1.0])]);

		instantiate_gvar(&mut font, &location, true)?;

		// check 'hyphen' coordinates changed
		assert_eq!(
			get_coordinates(&mut font, "hyphen")?,
			[
				(26.0, 259.0),
				(26.0, 286.0),
				(237.0, 286.0),
				(237.0, 259.0),
				(0.0, 0.0),
				(263.0, 0.0),
				(0.0, 536.0),
				(0.0, 0.0),
			]
		);
		// check 'minus' coordinates (i.e. component offset and phantom points) did _not_ change
		assert_eq!(get_coordinates(&mut font, "minus")?, minus_coords);

		assert_eq!(metrics(&mut font, "hyphen")?, ((263, 26), (536, 250)));
		// 'minus' left sidebearing and top sidebearing changed
		assert_eq!(metrics(&mut font, "minus")?, ((422, 26), (536, 250)));
		Ok(())
	}
}

mod instantiate_cvar_test {
	use fontoxide_ttlib::tables::{Cvar, Cvt};
	use fontoxide_varlib::instancer::instantiate_cvar;

	use super::*;

	#[test]
	fn test_pin_and_drop_axis() -> TestResult {
		let cases: [(&str, f64, [i16; 4]); 4] = [
			("wght", -1.0, [500, -400, 150, 250]),
			("wdth", -1.0, [500, -400, 180, 200]),
			("wght", -0.5, [500, -400, 165, 250]),
			("wdth", -0.3, [500, -400, 180, 235]),
		];
		for (axis, value, expected) in cases {
			let mut font = varfont()?;
			instantiate_cvar(&mut font, &normalized(&[(axis, &[value])]))?;
			assert_eq!(font.require::<Cvt>()?.values, expected);
			// check that the pinned axis has been dropped from cvar
			assert!(
				!font
					.require::<Cvar>()?
					.variations
					.iter()
					.any(|t| t.axes.contains_key(axis))
			);
		}
		Ok(())
	}

	#[test]
	fn test_full_instance() -> TestResult {
		let mut font = varfont()?;
		instantiate_cvar(
			&mut font,
			&normalized(&[("wght", &[-0.5]), ("wdth", &[-0.5])]),
		)?;
		assert_eq!(font.require::<Cvt>()?.values, [500, -400, 165, 225]);
		assert!(!font.contains(Tag::new(b"cvar")));
		Ok(())
	}
}

mod instantiate_mvar_test {
	use fontoxide_ttlib::Font;
	use fontoxide_ttlib::beyond64k::{ConvertOptions, upper_tables};
	use fontoxide_ttlib::ot::types as ot;
	use fontoxide_ttlib::ot::var_store::{build_var_data, build_var_region_list, build_var_store};
	use fontoxide_ttlib::tables::{Hhea, Mvar, Os2, Post, Vhea};
	use fontoxide_varlib::instancer::instantiate_mvar;

	use super::*;

	/// `getattr(varfont[table_tag], item_name)` of `MVAR_ENTRIES[tag]`.
	fn metric(font: &mut Font, tag: &str) -> TestResult<i64> {
		Ok(match tag {
			"hcrs" => font.require::<Hhea>()?.caret_slope_rise.into(),
			"hcrn" => font.require::<Hhea>()?.caret_slope_run.into(),
			"hcof" => font.require::<Hhea>()?.caret_offset.into(),
			"vasc" => font.require::<Vhea>()?.ascent.into(),
			"vdsc" => font.require::<Vhea>()?.descent.into(),
			"vlgp" => font.require::<Vhea>()?.line_gap.into(),
			"vcrs" => font.require::<Vhea>()?.caret_slope_rise.into(),
			"vcrn" => font.require::<Vhea>()?.caret_slope_run.into(),
			"vcof" => font.require::<Vhea>()?.caret_offset.into(),
			"strs" => font.require::<Os2>()?.y_strikeout_size.into(),
			"xhgt" => font.require::<Os2>()?.sx_height.into(),
			"hasc" => font.require::<Os2>()?.s_typo_ascender.into(),
			"undo" => font.require::<Post>()?.underline_position.into(),
			"unds" => font.require::<Post>()?.underline_thickness.into(),
			_ => return Err(format!("unknown metric {tag}").into()),
		})
	}

	#[test]
	fn test_metric_header_companions() -> TestResult {
		for value_tag in [
			"hcrs", "hcrn", "hcof", "vasc", "vdsc", "vlgp", "vcrs", "vcrn", "vcof",
		] {
			for uppercase in [false, true] {
				for fully_instanced in [false, true] {
					let mut font = varfont()?;
					if uppercase {
						upper_tables(&mut font, &ConvertOptions::default())?;
					}
					let original = metric(&mut font, value_tag)?;
					let mvar = font.require::<Mvar>()?;
					mvar.value_record = vec![ot::MetricsValueRecord {
						value_tag: tag(value_tag),
						var_idx: 0,
						..Default::default()
					}];
					mvar.value_record_count = Some(1);
					mvar.var_store = Some(Box::new(build_var_store(
						build_var_region_list(
							&[region(&[("wght", (0.0, 1.0, 1.0))])],
							&["wght".to_owned(), "wdth".to_owned()],
						)?,
						vec![build_var_data(vec![0], vec![vec![20]], true)],
					)));
					let mut location = vec![("wght", pin(900.0))];
					if fully_instanced {
						location.push(("wdth", pin(100.0)));
					}
					let mut instance = instantiated(&font, &location)?;
					assert_eq!(metric(&mut instance, value_tag)?, original + 20);
					let mut reloaded = reload(&mut instance)?;
					assert_eq!(metric(&mut reloaded, value_tag)?, original + 20);
					// The metrics are those of the family of the font.
					assert_eq!(reloaded.require::<Hhea>()?.extended, uppercase);
				}
			}
		}
		Ok(())
	}

	#[test]
	fn test_pin_and_drop_axis() -> TestResult {
		let cases: [(&str, f64, [i64; 4]); 6] = [
			("wght", 1.0, [100, -200, 150, 530]),
			("wght", 0.5, [75, -150, 100, 515]),
			("wght", 0.0, [50, -100, 50, 500]),
			("wdth", -1.0, [20, -100, 50, 500]),
			("wdth", -0.5, [35, -100, 50, 500]),
			("wdth", 0.0, [50, -100, 50, 500]),
		];
		for (axis, value, expected) in cases {
			let mut font = varfont()?;
			let mvar = font.require::<Mvar>()?;
			let store = mvar.var_store.as_ref().ok_or("no VarStore")?;
			// initially we have two VarData: the first contains deltas associated with 3
			// regions: 1 with only wght, 1 with only wdth, and 1 with both wght and wdth
			assert_eq!(store.var_data.len(), 2);
			let list = store.var_region_list.as_ref().ok_or("no VarRegionList")?;
			assert_eq!(list.region_count, Some(3));
			let data = store.var_data[0].as_ref().ok_or("no VarData")?;
			assert_eq!(data.var_region_count, Some(3));
			assert!(data.item.iter().all(|item| item.len() == 3));
			// The second VarData has deltas associated only with 1 region (wght only).
			let data = store.var_data[1].as_ref().ok_or("no VarData")?;
			assert_eq!(data.var_region_count, Some(1));
			assert!(data.item.iter().all(|item| item.len() == 1));

			instantiate_mvar(&mut font, &normalized(&[(axis, &[value])]))?;

			for (mvar_tag, expected) in ["strs", "undo", "unds", "xhgt"].iter().zip(expected) {
				assert_eq!(metric(&mut font, mvar_tag)?, expected, "{mvar_tag}");
			}

			// check that regions and accompanying deltas have been dropped
			let mvar = font.require::<Mvar>()?;
			let store = mvar.var_store.as_ref().ok_or("no VarStore")?;
			let list = store.var_region_list.as_ref().ok_or("no VarRegionList")?;
			let num_regions_left = list.region.len();
			assert!(num_regions_left < 3);
			assert_eq!(list.region_count, Some(num_regions_left as u32));
			let data = store.var_data[0].as_ref().ok_or("no VarData")?;
			assert_eq!(data.var_region_count, Some(num_regions_left as u32));
			// VarData subtables have been merged
			assert_eq!(store.var_data.len(), 1);
		}
		Ok(())
	}

	#[test]
	fn test_full_instance() -> TestResult {
		let cases: [(f64, f64, [i64; 4], bool); 5] = [
			(1.0, 0.0, [100, -200, 150, 1100], true),
			(0.0, -1.0, [20, -100, 50, 1000], true),
			(0.5, -0.5, [55, -145, 95, 1050], true),
			(1.0, -1.0, [50, -180, 130, 1100], true),
			(1.0, 0.0, [100, -200, 150, 1100], false),
		];
		for (wght, wdth, expected, sync_vmetrics) in cases {
			let mut font = varfont()?;
			let location = normalized(&[("wght", &[wght]), ("wdth", &[wdth])]);

			// check vertical metrics are in sync before...
			let hhea = font.require::<Hhea>()?.clone();
			let os2 = font.require::<Os2>()?;
			if sync_vmetrics {
				assert_eq!(os2.s_typo_ascender, hhea.ascent);
				assert_eq!(os2.s_typo_descender, hhea.descent);
				assert_eq!(os2.s_typo_line_gap, hhea.line_gap);
			} else {
				// force them not to be in sync
				os2.s_typo_descender -= 100;
				os2.s_typo_line_gap += 200;
			}

			instantiate_mvar(&mut font, &location)?;

			for (mvar_tag, expected) in ["strs", "undo", "unds", "hasc"].iter().zip(expected) {
				assert_eq!(metric(&mut font, mvar_tag)?, expected, "{mvar_tag}");
			}

			// ... as well as after instancing, but only if they were already
			// https://github.com/fonttools/fonttools/issues/3297
			let hhea = font.require::<Hhea>()?.clone();
			let os2 = font.require::<Os2>()?;
			if sync_vmetrics {
				assert_eq!(os2.s_typo_ascender, hhea.ascent);
				assert_eq!(os2.s_typo_descender, hhea.descent);
				assert_eq!(os2.s_typo_line_gap, hhea.line_gap);
			} else {
				assert_ne!(os2.s_typo_descender, hhea.descent);
				assert_ne!(os2.s_typo_line_gap, hhea.line_gap);
			}

			assert!(!font.contains(Tag::new(b"MVAR")));
		}
		Ok(())
	}
}

mod instantiate_hvar_test {
	use fontoxide_ttlib::ot::var_store::region_support;
	use fontoxide_ttlib::tables::Hvar;
	use fontoxide_varlib::instancer::instantiate_hvar;

	use super::*;

	// the 'expected_deltas' below refer to the VarData item deltas for the "hyphen" glyph in
	// the PartialInstancerTest-VF.ttx test font, that are left after partial instancing
	#[test]
	fn test_partial_instance() -> TestResult {
		let wdth_region = [("wdth", (-1.0, -1.0, 0.0))];
		let cases: Vec<(&str, f64, Vec<Region>, Vec<i64>)> = vec![
			("wght", -1.0, vec![region(&wdth_region)], vec![-59]),
			("wght", 0.0, vec![region(&wdth_region)], vec![-48]),
			("wght", 1.0, vec![region(&wdth_region)], vec![7]),
			(
				"wdth",
				-1.0,
				vec![
					region(&[("wght", (-1.0, -1.0, 0.0))]),
					region(&[("wght", (0.0, 0.6099854, 1.0))]),
					region(&[("wght", (0.6099854, 1.0, 1.0))]),
				],
				vec![-11, 31, 51],
			),
			(
				"wdth",
				0.0,
				vec![region(&[("wght", (0.6099854, 1.0, 1.0))])],
				vec![-4],
			),
		];
		for (axis, value, expected_regions, expected_deltas) in cases {
			let mut font = varfont()?;
			instantiate_hvar(&mut font, &normalized(&[(axis, &[value])]))?;

			assert!(font.contains(Tag::new(b"HVAR")));
			let notdef = font.glyph_id(".notdef")?;
			let space = font.glyph_id("space")?;
			let hyphen = font.glyph_id("hyphen")?;
			let mut fvar_axes = axis_tags(&mut font)?;
			fvar_axes.retain(|t| t != axis);
			let hvar = font.require::<Hvar>()?;
			let var_store = hvar.var_store.as_ref().ok_or("no VarStore")?;

			let regions = &var_store.var_region_list.as_ref().ok_or("no list")?.region;
			assert_eq!(regions.len(), expected_regions.len());
			for (region, expected) in regions.iter().zip(&expected_regions) {
				let support = region_support(region, &fvar_axes)?;
				assert_eq!(
					support.keys().collect::<Vec<_>>(),
					expected.keys().collect::<Vec<_>>()
				);
				for (axis, s) in &support {
					let e = expected[axis];
					assert!(approx(s.0, e.0) && approx(s.1, e.1) && approx(s.2, e.2));
				}
			}

			assert_eq!(var_store.var_data.len(), 1);
			let data = var_store.var_data[0].as_ref().ok_or("no VarData")?;
			assert_eq!(data.item_count, Some(2));

			let map = hvar.adv_width_map.as_ref().ok_or("no AdvWidthMap")?;
			assert_eq!(map.mapping[&notdef], map.mapping[&space]);
			let item = |var_idx: u64| -> TestResult<&Vec<i64>> {
				let data = var_store.var_data[(var_idx >> 16) as usize]
					.as_ref()
					.ok_or("no VarData")?;
				Ok(&data.item[(var_idx & 0xFFFF) as usize])
			};
			// these glyphs have no metrics variations in the test font
			assert_eq!(
				*item(map.mapping[&notdef])?,
				vec![0; data.var_region_count.ok_or("no count")? as usize]
			);
			assert_eq!(*item(map.mapping[&hyphen])?, expected_deltas);
		}
		Ok(())
	}

	#[test]
	fn test_full_instance() -> TestResult {
		let mut font = varfont()?;
		instantiate_hvar(
			&mut font,
			&normalized(&[("wght", &[0.0]), ("wdth", &[0.0])]),
		)?;
		assert!(!font.contains(Tag::new(b"HVAR")));
		Ok(())
	}

	#[test]
	fn test_partial_instance_keep_empty_table() -> TestResult {
		// Append an additional dummy axis to fvar, for which the current HVAR table in our
		// test 'varfont' contains no variation data. Instancing the other two wght and wdth
		// axes should leave HVAR table empty, to signal there are variations to the glyph's
		// advance widths.
		let mut font = varfont()?;
		font.require::<Fvar>()?.axes.push(Axis::new(tag("TEST")));

		instantiate_hvar(
			&mut font,
			&normalized(&[("wght", &[0.0]), ("wdth", &[0.0])]),
		)?;

		assert!(font.contains(Tag::new(b"HVAR")));
		let hvar = font.require::<Hvar>()?;
		let store = hvar.var_store.as_ref().ok_or("no VarStore")?;
		let list = store.var_region_list.as_ref().ok_or("no list")?;
		assert_eq!(list.region_count, Some(0));
		assert!(list.region.is_empty());
		assert_eq!(list.region_axis_count, Some(1));
		Ok(())
	}
}

mod instantiate_vvar_test {
	use std::collections::BTreeMap;

	use fontoxide_ttlib::ot::types as ot;
	use fontoxide_ttlib::ot::varmap::VarIdxMap;
	use fontoxide_ttlib::tables::{Hmtx, Hvar, Vmtx, Vorg, Vvar};
	use fontoxide_ttlib::var::{Location, VarStoreInstancer};
	use fontoxide_ttlib::{Font, TableType};
	use fontoxide_varlib::instancer::{instantiate_hvar, instantiate_vvar};

	use super::*;

	/// The fixture, with its parameter.
	fn fonts() -> TestResult<Vec<Font>> {
		Ok(vec![fixture("vvar_1")?, fixture("vvar_2")?])
	}

	fn glyph_ids(font: &mut Font) -> TestResult<Vec<u32>> {
		Ok((0..font.glyph_order()?.names().len() as u32).collect())
	}

	fn var_idx_map(indices: &[u64]) -> Option<Box<VarIdxMap>> {
		Some(Box::new(VarIdxMap {
			mapping: indices
				.iter()
				.enumerate()
				.map(|(gid, idx)| (gid as u32, *idx))
				.collect(),
		}))
	}

	fn reloaded_vorg(font: &mut Font) -> TestResult<Vorg> {
		let data = font.table_data(Tag::new(b"VORG"))?;
		Ok(Vorg::decompile(&data, font)?)
	}

	fn metrics_of(font: &mut Font) -> TestResult<BTreeMap<u32, (u16, i16)>> {
		Ok(font.require::<Vmtx>()?.metrics.clone())
	}

	#[test]
	fn test_vertical_origins() -> TestResult {
		type Case<'a> = (
			&'a [(&'a str, &'a [f64])],
			[i16; 4],
			Option<(&'a str, f64)>,
			[f64; 4],
		);
		let cases: [Case<'_>; 3] = [
			(
				&[("wght", &[1.0]), ("wdth", &[0.0])],
				[810, 900, 800, 800],
				None,
				[0.0; 4],
			),
			(
				&[("wght", &[1.0])],
				[810, 900, 800, 800],
				Some(("wdth", -1.0)),
				[810.0, 930.0, 800.0, 800.0],
			),
			(
				&[("wght", &[0.5, 0.5, 1.0]), ("wdth", &[0.0])],
				[805, 850, 825, 800],
				Some(("wght", 1.0)),
				[810.0, 900.0, 800.0, 800.0],
			),
		];
		for (limits, expected, remaining_location, remaining_origins) in cases {
			for mut font in fonts()? {
				let limits = normalized(limits);
				instantiate_vvar(&mut font, &limits)?;

				let glyphs = glyph_ids(&mut font)?;
				let vorg = font.require::<Vorg>()?;
				let origins: Vec<i16> = glyphs.iter().map(|g| vorg.get(*g)).collect();
				assert_eq!(origins, expected);
				assert_eq!(vorg.default_vert_origin_y, 800);
				assert!(vorg.v_origin_records.values().all(|v| *v != 800));
				let origins_f: Vec<f64> = origins.iter().map(|v| f64::from(*v)).collect();
				let metrics = metrics_of(&mut font)?;
				assert!(metrics.values().all(|m| *m == (1000, 100)));
				assert_eq!(metrics.len(), glyphs.len());

				let reloaded = reloaded_vorg(&mut font)?;
				let origins: Vec<i16> = glyphs.iter().map(|g| reloaded.get(*g)).collect();
				assert_eq!(origins, expected);

				match remaining_location {
					None => assert!(!font.contains(Tag::new(b"VVAR"))),
					Some((axis, value)) => {
						let pinned = limits.pinned_location();
						let mut remaining_axes = axis_tags(&mut font)?;
						remaining_axes.retain(|t| !pinned.contains_key(t));
						let vvar = font.require::<Vvar>()?;
						let location: Location = [(axis.to_owned(), value)].into_iter().collect();
						let evaluator = VarStoreInstancer::new(
							vvar.var_store.as_deref(),
							&remaining_axes,
							location,
						);
						let map = vvar.v_org_map.as_ref().ok_or("no VOrgMap")?;
						let actual = glyphs
							.iter()
							.zip(origins_f.iter())
							.map(|(g, o)| Ok(o + evaluator.get(map.mapping[g])?))
							.collect::<TestResult<Vec<f64>>>()?;
						assert_eq!(actual, remaining_origins);
					}
				}
			}
		}
		Ok(())
	}

	fn store_item(store: &mut ot::VarStore, row: usize) -> &mut Vec<i64> {
		&mut store.var_data[0].as_mut().expect("VarData").item[row]
	}

	#[test]
	fn test_uniform_origins_and_half_unit_rounding() -> TestResult {
		for (delta, rounded) in [(37, 19), (-37, -18)] {
			for mut font in fonts()? {
				let glyphs = glyph_ids(&mut font)?;
				font.require::<Vorg>()?.v_origin_records.clear();
				let vvar = font.require::<Vvar>()?;
				*store_item(vvar.var_store.as_mut().ok_or("no VarStore")?, 1) = vec![delta, 0];
				vvar.v_org_map = var_idx_map(&[1; 4]);
				vvar.adv_height_map = var_idx_map(&[1; 4]);
				instantiate_vvar(
					&mut font,
					&normalized(&[("wght", &[0.5]), ("wdth", &[0.0])]),
				)?;
				let vorg = font.require::<Vorg>()?;
				assert_eq!(vorg.default_vert_origin_y, 800 + rounded);
				assert!(vorg.v_origin_records.is_empty());
				for glyph in &glyphs {
					assert_eq!(vorg.get(*glyph), 800 + rounded);
				}
				let metrics = metrics_of(&mut font)?;
				let advance = 1000 + i32::from(rounded);
				assert!(metrics.values().all(|m| *m == (advance as u16, 100)));
				assert_eq!(metrics.len(), glyphs.len());
				let reloaded = reloaded_vorg(&mut font)?;
				assert_eq!(reloaded.default_vert_origin_y, 800 + rounded);
				assert!(reloaded.v_origin_records.is_empty());
			}
		}
		Ok(())
	}

	#[test]
	fn test_hvar_half_unit_rounding() -> TestResult {
		for (delta, rounded) in [(37, 19), (-37, -18)] {
			for mut font in fonts()? {
				let glyphs = glyph_ids(&mut font)?;
				let mut hmtx = Hmtx::default();
				for glyph in &glyphs {
					hmtx.metrics.insert(*glyph, (1000, 100));
				}
				font.insert(Box::new(hmtx));
				let mut hvar = Hvar::default();
				hvar.version = 0x10000;
				hvar.var_store = font.require::<Vvar>()?.var_store.clone();
				*store_item(hvar.var_store.as_mut().ok_or("no VarStore")?, 1) = vec![delta, 0];
				hvar.adv_width_map = var_idx_map(&[1; 4]);
				font.insert(Box::new(hvar));
				instantiate_hvar(
					&mut font,
					&normalized(&[("wght", &[0.5]), ("wdth", &[0.0])]),
				)?;
				let metrics = &font.require::<Hmtx>()?.metrics;
				let advance = (1000 + rounded) as u16;
				assert!(metrics.values().all(|m| *m == (advance, 100)));
				assert_eq!(metrics.len(), glyphs.len());
			}
		}
		Ok(())
	}

	#[test]
	fn test_no_origin_mapping() -> TestResult {
		for mut font in fonts()? {
			font.require::<Vvar>()?.v_org_map = None;
			instantiate_vvar(
				&mut font,
				&normalized(&[("wght", &[1.0]), ("wdth", &[0.0])]),
			)?;
			let b = font.glyph_id("b")?;
			let vorg = font.require::<Vorg>()?;
			assert_eq!(vorg.default_vert_origin_y, 800);
			assert_eq!(
				vorg.v_origin_records,
				[(b, 850)].into_iter().collect::<BTreeMap<_, _>>()
			);
		}
		Ok(())
	}
}

fn wght_wdth() -> Vec<String> {
	vec!["wght".to_owned(), "wdth".to_owned()]
}

const WGHT_NEG: (f64, f64, f64) = (-1.0, -1.0, 0.0);
const WGHT_MID: (f64, f64, f64) = (0.0, 0.5, 1.0);
const WGHT_POS: (f64, f64, f64) = (0.5, 1.0, 1.0);
const WGHT_FULL: (f64, f64, f64) = (0.0, 1.0, 1.0);
const WDTH_NEG: (f64, f64, f64) = (-1.0, -1.0, 0.0);

mod instantiate_item_variation_store_test {
	use fontoxide_ttlib::ot::var_store::{
		build_var_data, build_var_region, build_var_region_list, build_var_store, region_support,
	};
	use fontoxide_varlib::instancer::instantiate_item_variation_store;

	use super::*;

	#[test]
	fn test_var_region_get_support() -> TestResult {
		let axis_order = ["wght", "wdth", "opsz"].map(str::to_owned);
		let region_axes = region(&[("wdth", WDTH_NEG), ("wght", WGHT_FULL)]);
		let region = build_var_region(&region_axes, &axis_order)?;
		assert_eq!(region.var_region_axis.len(), 3);
		assert_eq!(region.var_region_axis[2].peak_coord, 0.0);
		assert_eq!(region_support(&region, &axis_order)?, region_axes);
		Ok(())
	}

	#[test]
	fn test_instantiate_default_deltas() -> TestResult {
		type Case<'a> = (&'a [(&'a str, &'a [f64])], [[f64; 2]; 2], u32);
		let cases: [Case<'_>; 7] = [
			(&[("wght", &[0.0])], [[0.0, 0.0], [0.0, 0.0]], 1),
			(&[("wght", &[0.25])], [[50.0, 50.0], [0.0, 0.0]], 1),
			(&[("wdth", &[0.0])], [[0.0, 0.0], [0.0, 0.0]], 3),
			(&[("wdth", &[-0.75])], [[0.0, 0.0], [75.0, 75.0]], 3),
			(
				&[("wght", &[0.0]), ("wdth", &[0.0])],
				[[0.0, 0.0], [0.0, 0.0]],
				0,
			),
			(
				&[("wght", &[0.25]), ("wdth", &[0.0])],
				[[50.0, 50.0], [0.0, 0.0]],
				0,
			),
			(
				&[("wght", &[0.0]), ("wdth", &[-0.75])],
				[[0.0, 0.0], [75.0, 75.0]],
				0,
			),
		];
		for (location, expected_deltas, num_regions) in cases {
			let mut var_store = build_var_store(
				build_var_region_list(
					&[
						region(&[("wght", WGHT_NEG)]),
						region(&[("wght", WGHT_MID)]),
						region(&[("wght", WGHT_POS)]),
						region(&[("wdth", WDTH_NEG)]),
						region(&[("wght", WGHT_NEG), ("wdth", WDTH_NEG)]),
						region(&[("wght", WGHT_MID), ("wdth", WDTH_NEG)]),
						region(&[("wght", WGHT_POS), ("wdth", WDTH_NEG)]),
					],
					&wght_wdth(),
				)?,
				vec![
					build_var_data(vec![0, 1, 2], vec![vec![100; 3]; 2], true),
					build_var_data(vec![3, 4, 5, 6], vec![vec![100; 4]; 2], true),
				],
			);
			let default_deltas = instantiate_item_variation_store(
				&mut var_store,
				&wght_wdth(),
				&normalized(location),
			)?;

			let mut sorted: Vec<_> = default_deltas.0.iter().map(|(k, v)| (*k, *v)).collect();
			sorted.sort_by_key(|e| e.0);
			let mut default_delta_array: Vec<Vec<f64>> = Vec::new();
			for (var_idx, delta) in sorted {
				if var_idx == NO_VARIATION_INDEX {
					continue;
				}
				let (major, minor) = ((var_idx >> 16) as usize, (var_idx & 0xFFFF) as usize);
				if major == default_delta_array.len() {
					default_delta_array.push(Vec::new());
				}
				assert_eq!(default_delta_array[major].len(), minor);
				default_delta_array[major].push(delta);
			}
			assert_eq!(default_delta_array, expected_deltas);
			let list = var_store.var_region_list.as_ref().ok_or("no list")?;
			assert_eq!(list.region_count, Some(num_regions));
		}
		Ok(())
	}
}

mod tuple_var_store_adapter_test {
	use fontoxide_ttlib::ot::var_store::{
		build_var_data, build_var_region_list, build_var_store, region_support,
	};
	use fontoxide_ttlib::tables::tuple_variation::TupleVariation;
	use fontoxide_varlib::instancer::TupleVarStore;

	use super::*;

	fn assert_same_data(actual: &[Vec<TupleVariation>], expected: &[Vec<TupleVariation>]) {
		assert_eq!(actual.len(), expected.len());
		for (a, e) in actual.iter().zip(expected) {
			assert_eq!(a.len(), e.len());
			for (a, e) in a.iter().zip(e) {
				assert!(same_variation(a, e), "{a:?} != {e:?}");
			}
		}
	}

	#[test]
	fn test_instantiate() -> TestResult {
		let regions = vec![
			region(&[("wght", WGHT_NEG)]),
			region(&[("wght", WGHT_FULL)]),
			region(&[("wdth", WDTH_NEG)]),
			region(&[("wght", WGHT_NEG), ("wdth", WDTH_NEG)]),
			region(&[("wght", WGHT_FULL), ("wdth", WDTH_NEG)]),
		];
		let tuple_var_data = vec![
			vec![
				tuple_variation(&[("wght", WGHT_NEG)], &[10, 70]),
				tuple_variation(&[("wght", WGHT_FULL)], &[30, 90]),
				tuple_variation(&[("wght", WGHT_NEG), ("wdth", WDTH_NEG)], &[-40, -100]),
				tuple_variation(&[("wght", WGHT_FULL), ("wdth", WDTH_NEG)], &[-60, -120]),
			],
			vec![
				tuple_variation(&[("wdth", WDTH_NEG)], &[5, 45]),
				tuple_variation(&[("wght", WGHT_NEG), ("wdth", WDTH_NEG)], &[-15, -55]),
				tuple_variation(&[("wght", WGHT_FULL), ("wdth", WDTH_NEG)], &[-35, -75]),
			],
		];
		let mut adapter = TupleVarStore {
			regions,
			axis_order: wght_wdth(),
			tuple_var_data,
			item_counts: vec![2, 2],
		};

		let default_delta_array = adapter.instantiate(&normalized(&[("wght", &[0.5])]), true)?;

		assert_eq!(default_delta_array, [[15.0, 45.0], [0.0, 0.0]]);
		assert_eq!(adapter.regions, [region(&[("wdth", WDTH_NEG)])]);
		assert_same_data(
			&adapter.tuple_var_data,
			&[
				vec![tuple_variation(&[("wdth", WDTH_NEG)], &[-30, -60])],
				vec![tuple_variation(&[("wdth", WDTH_NEG)], &[-12, 8])],
			],
		);
		Ok(())
	}

	#[test]
	fn test_rebuild_regions() {
		let regions = vec![
			region(&[("wght", WGHT_NEG)]),
			region(&[("wght", WGHT_FULL)]),
			region(&[("wdth", WDTH_NEG)]),
			region(&[("wght", WGHT_NEG), ("wdth", WDTH_NEG)]),
			region(&[("wght", WGHT_FULL), ("wdth", WDTH_NEG)]),
		];
		let variations: Vec<TupleVariation> = regions
			.iter()
			.map(|r| TupleVariation {
				axes: r.clone(),
				coordinates: vec![value_delta(100)],
			})
			.collect();
		let tuple_var_data = vec![variations[..3].to_vec(), variations[3..].to_vec()];
		let mut adapter = TupleVarStore {
			regions: regions.clone(),
			axis_order: wght_wdth(),
			tuple_var_data,
			item_counts: vec![1, 1],
		};

		adapter.rebuild_regions();

		assert_eq!(adapter.regions, regions);

		adapter.tuple_var_data[0].remove(2);
		adapter.tuple_var_data[1][0].axes = region(&[("wght", (-1.0, -0.5, 0.0))]);
		adapter.tuple_var_data[1][1].axes = region(&[("wght", WGHT_MID)]);

		adapter.rebuild_regions();

		assert_eq!(
			adapter.regions,
			[
				region(&[("wght", WGHT_NEG)]),
				region(&[("wght", WGHT_FULL)]),
				region(&[("wght", (-1.0, -0.5, 0.0))]),
				region(&[("wght", WGHT_MID)]),
			]
		);
	}

	#[test]
	fn test_roundtrip() -> TestResult {
		let regions = vec![
			region(&[("wght", WGHT_NEG)]),
			region(&[("wght", WGHT_MID)]),
			region(&[("wght", WGHT_POS)]),
			region(&[("wdth", WDTH_NEG)]),
			region(&[("wght", WGHT_NEG), ("wdth", WDTH_NEG)]),
			region(&[("wght", WGHT_MID), ("wdth", WDTH_NEG)]),
			region(&[("wght", WGHT_POS), ("wdth", WDTH_NEG)]),
		];
		let axis_order = wght_wdth();

		let item_var_store = build_var_store(
			build_var_region_list(&regions, &axis_order)?,
			vec![
				build_var_data(
					vec![0, 1, 2, 4, 5, 6],
					vec![
						vec![10, -20, 30, -40, 50, -60],
						vec![70, -80, 90, -100, 110, -120],
					],
					true,
				),
				build_var_data(
					vec![3, 4, 5, 6],
					vec![vec![5, -15, 25, -35], vec![45, -55, 65, -75]],
					true,
				),
			],
		);

		let adapter = TupleVarStore::from_item_var_store(&item_var_store, &axis_order)?;

		assert_same_data(
			&adapter.tuple_var_data,
			&[
				vec![
					tuple_variation(&[("wght", WGHT_NEG)], &[10, 70]),
					tuple_variation(&[("wght", WGHT_MID)], &[-20, -80]),
					tuple_variation(&[("wght", WGHT_POS)], &[30, 90]),
					tuple_variation(&[("wght", WGHT_NEG), ("wdth", WDTH_NEG)], &[-40, -100]),
					tuple_variation(&[("wght", WGHT_MID), ("wdth", WDTH_NEG)], &[50, 110]),
					tuple_variation(&[("wght", WGHT_POS), ("wdth", WDTH_NEG)], &[-60, -120]),
				],
				vec![
					tuple_variation(&[("wdth", WDTH_NEG)], &[5, 45]),
					tuple_variation(&[("wght", WGHT_NEG), ("wdth", WDTH_NEG)], &[-15, -55]),
					tuple_variation(&[("wght", WGHT_MID), ("wdth", WDTH_NEG)], &[25, 65]),
					tuple_variation(&[("wght", WGHT_POS), ("wdth", WDTH_NEG)], &[-35, -75]),
				],
			],
		);
		let counts: Vec<usize> = item_var_store
			.var_data
			.iter()
			.flatten()
			.map(|d| d.item_count.unwrap_or_default() as usize)
			.collect();
		assert_eq!(adapter.item_counts, counts);
		assert_eq!(adapter.regions, regions);
		assert_eq!(adapter.axis_order, axis_order);

		let item_var_store2 = adapter.as_item_var_store(true)?;

		let list = item_var_store2.var_region_list.as_ref().ok_or("no list")?;
		let supports = list
			.region
			.iter()
			.map(|r| Ok(region_support(r, &axis_order)?))
			.collect::<TestResult<Vec<_>>>()?;
		assert_eq!(supports, regions);

		assert_eq!(item_var_store2.var_data_count, Some(2));
		let data = item_var_store2.var_data[0].as_ref().ok_or("no VarData")?;
		assert_eq!(data.var_region_index, [0, 1, 2, 4, 5, 6]);
		assert_eq!(
			data.item,
			[
				vec![10, -20, 30, -40, 50, -60],
				vec![70, -80, 90, -100, 110, -120],
			]
		);
		let data = item_var_store2.var_data[1].as_ref().ok_or("no VarData")?;
		assert_eq!(data.var_region_index, [3, 4, 5, 6]);
		assert_eq!(data.item, [vec![5, -15, 25, -35], vec![45, -55, 65, -75]]);
		Ok(())
	}
}

mod instantiate_avar_test {
	use std::collections::BTreeMap;

	use fontoxide_ttlib::Font;
	use fontoxide_ttlib::tables::Name;
	use fontoxide_ttlib::tables::avar::SegmentMap;
	use fontoxide_varlib::instancer::{AxisTriple, instantiate_avar, valid_avar_segment_map};

	use super::*;

	/// `instantiateAvar(varfont, limits, limits.normalize(varfont))`.
	fn instantiate(font: &mut Font, pairs: &[(&str, Option<AxisTriple>)]) -> TestResult {
		let limits = user_limits(font, pairs)?;
		let normalized = limits.normalize(font, true)?;
		instantiate_avar(font, &limits, &normalized, None)?;
		Ok(())
	}

	#[test]
	fn test_pin_and_drop_axis() -> TestResult {
		for axis in ["wght", "wdth"] {
			let mut font = varfont()?;
			instantiate(&mut font, &[(axis, pin(0.0))])?;
			assert!(!font.require::<Avar>()?.segments.contains_key(&tag(axis)));
		}
		Ok(())
	}

	#[test]
	fn test_full_instance() -> TestResult {
		let mut font = varfont()?;
		instantiate(&mut font, &[("wght", pin(0.0)), ("wdth", pin(0.0))])?;
		assert!(!font.contains(Tag::new(b"avar")));
		Ok(())
	}

	fn quantize_f2dot14(mapping: &[(f64, f64)]) -> SegmentMap {
		mapping
			.iter()
			.map(|(k, v)| (f2dot14(*k), f2dot14(*v)))
			.collect()
	}

	// the following values come from NotoSans-VF.ttf
	const DFLT_WGHT_MAPPING: [(f64, f64); 9] = [
		(-1.0, -1.0),
		(-0.6667, -0.7969),
		(-0.3333, -0.5),
		(0.0, 0.0),
		(0.2, 0.18),
		(0.4, 0.38),
		(0.6, 0.61),
		(0.8, 0.79),
		(1.0, 1.0),
	];

	const DFLT_WDTH_MAPPING: [(f64, f64); 5] = [
		(-1.0, -1.0),
		(-0.6667, -0.7),
		(-0.3333, -0.36664),
		(0.0, 0.0),
		(1.0, 1.0),
	];

	/// The `varfont` fixture of the class.
	fn avar_varfont() -> Font {
		let mut font = Font::new();
		font.insert(Box::new(Name::default()));
		let mut fvar = Fvar::default();
		for (axis_tag, minimum, default, maximum) in
			[("wght", 100.0, 400.0, 900.0), ("wdth", 62.5, 100.0, 100.0)]
		{
			let mut axis = Axis::new(tag(axis_tag));
			axis.min_value = minimum;
			axis.default_value = default;
			axis.max_value = maximum;
			fvar.axes.push(axis);
		}
		font.insert(Box::new(fvar));
		let mut avar = Avar::default();
		avar.segments
			.insert(tag("wght"), quantize_f2dot14(&DFLT_WGHT_MAPPING));
		avar.segments
			.insert(tag("wdth"), quantize_f2dot14(&DFLT_WDTH_MAPPING));
		font.insert(Box::new(avar));
		font
	}

	fn sorted_map(map: &SegmentMap) -> Vec<(f64, f64)> {
		let mut map = map.clone();
		map.sort_by(|a, b| a.0.total_cmp(&b.0));
		map
	}

	#[test]
	fn test_limit_axes() -> TestResult {
		let wght: &[(f64, f64)] = &DFLT_WGHT_MAPPING;
		let wdth: &[(f64, f64)] = &DFLT_WDTH_MAPPING;
		type Case<'a> = (&'a str, (f64, f64), &'a [(f64, f64)], &'a [(f64, f64)]);
		let cases: [Case<'_>; 11] = [
			("wght", (100.0, 900.0), wght, wdth),
			(
				"wght",
				(400.0, 900.0),
				&[
					(-1.0, -1.0),
					(0.0, 0.0),
					(0.2, 0.18),
					(0.4, 0.38),
					(0.6, 0.61),
					(0.8, 0.79),
					(1.0, 1.0),
				],
				wdth,
			),
			(
				"wght",
				(100.0, 400.0),
				&[
					(-1.0, -1.0),
					(-0.6667, -0.7969),
					(-0.3333, -0.5),
					(0.0, 0.0),
					(1.0, 1.0),
				],
				wdth,
			),
			(
				"wght",
				(400.0, 800.0),
				&[
					(-1.0, -1.0),
					(0.0, 0.0),
					(0.25, 0.22784),
					(0.50006, 0.48103),
					(0.75, 0.77214),
					(1.0, 1.0),
				],
				wdth,
			),
			(
				"wght",
				(400.0, 700.0),
				&[
					(-1.0, -1.0),
					(0.0, 0.0),
					(0.3334, 0.2951),
					(0.66675, 0.623),
					(1.0, 1.0),
				],
				wdth,
			),
			(
				"wght",
				(400.0, 600.0),
				&[(-1.0, -1.0), (0.0, 0.0), (0.5, 0.47363), (1.0, 1.0)],
				wdth,
			),
			(
				"wdth",
				(62.5, 100.0),
				wght,
				&[
					(-1.0, -1.0),
					(-0.6667, -0.7),
					(-0.3333, -0.36664),
					(0.0, 0.0),
					(1.0, 1.0),
				],
			),
			(
				"wdth",
				(70.0, 100.0),
				wght,
				&[
					(-1.0, -1.0),
					(-0.8334, -0.85364),
					(-0.4166, -0.44714),
					(0.0, 0.0),
					(1.0, 1.0),
				],
			),
			(
				"wdth",
				(75.0, 100.0),
				wght,
				&[(-1.0, -1.0), (-0.49994, -0.52374), (0.0, 0.0), (1.0, 1.0)],
			),
			(
				"wdth",
				(77.0, 100.0),
				wght,
				&[(-1.0, -1.0), (-0.54346, -0.56696), (0.0, 0.0), (1.0, 1.0)],
			),
			(
				"wdth",
				(87.5, 100.0),
				wght,
				&[(-1.0, -1.0), (0.0, 0.0), (1.0, 1.0)],
			),
		];
		for (axis, (minimum, maximum), expected_wght, expected_wdth) in cases {
			let mut font = avar_varfont();
			instantiate(&mut font, &[(axis, range(minimum, maximum))])?;

			let new_segments: BTreeMap<Tag, Vec<(f64, f64)>> = font
				.require::<Avar>()?
				.segments
				.iter()
				.map(|(t, m)| (*t, sorted_map(m)))
				.collect();
			let expected: BTreeMap<Tag, Vec<(f64, f64)>> = [
				(tag("wght"), quantize_f2dot14(expected_wght)),
				(tag("wdth"), quantize_f2dot14(expected_wdth)),
			]
			.into_iter()
			.collect();
			assert_eq!(new_segments, expected, "{axis} {minimum}:{maximum}");
		}
		Ok(())
	}

	#[test]
	fn test_drop_invalid_segment_map() -> TestResult {
		let cases: [&[(f64, f64)]; 3] = [
			// missing-required-maps-1
			&[(0.5, 0.5)],
			// missing-required-maps-2
			&[(-1.0, -1.0), (1.0, 1.0)],
			// retrograde-value-maps
			&[(-1.0, -1.0), (0.0, 0.0), (0.5, 0.5), (0.6, 0.4), (1.0, 1.0)],
		];
		for invalid_segment_map in cases {
			let mut font = avar_varfont();
			font.require::<Avar>()?
				.segments
				.insert(tag("wght"), invalid_segment_map.to_vec());

			let (text, result) =
				capture_warnings(|| instantiate(&mut font, &[("wght", range(100.0, 400.0))]));
			result?;

			assert!(text.contains("Invalid avar"), "{text}");
			assert!(!font.require::<Avar>()?.segments.contains_key(&tag("wght")));
		}
		Ok(())
	}

	#[test]
	fn test_is_valid_avar_segment_map() {
		assert!(valid_avar_segment_map("FOOO", &vec![]));
		assert!(valid_avar_segment_map(
			"FOOO",
			&vec![(-1.0, -1.0), (0.0, 0.0), (1.0, 1.0)]
		));
		assert!(valid_avar_segment_map(
			"FOOO",
			&vec![(-1.0, -1.0), (0.0, 0.0), (0.5, 0.5), (1.0, 1.0)]
		));
		assert!(valid_avar_segment_map(
			"FOOO",
			&vec![(-1.0, -1.0), (0.0, 0.0), (0.5, 0.5), (0.7, 0.5), (1.0, 1.0)]
		));
	}
}

mod instantiate_fvar_test {
	use fontoxide_ttlib::tables::Name;
	use fontoxide_varlib::instancer::{AxisTriple, instantiate_fvar};

	use super::*;

	#[test]
	fn test_pin_and_drop_axis() -> TestResult {
		let cases: [(&str, f64, &[&str]); 4] = [
			(
				"wght",
				400.0,
				&["Regular", "SemiCondensed", "Condensed", "ExtraCondensed"],
			),
			(
				"wght",
				100.0,
				&[
					"Thin",
					"SemiCondensed Thin",
					"Condensed Thin",
					"ExtraCondensed Thin",
				],
			),
			(
				"wdth",
				100.0,
				&[
					"Thin",
					"ExtraLight",
					"Light",
					"Regular",
					"Medium",
					"SemiBold",
					"Bold",
					"ExtraBold",
					"Black",
				],
			),
			// no named instance at pinned location
			("wdth", 90.0, &[]),
		];
		for (axis, value, instances_left) in cases {
			let mut font = varfont()?;
			let limits = user_limits(&mut font, &[(axis, pin(value))])?;

			instantiate_fvar(&mut font, &limits)?;

			let fvar = font.require::<Fvar>()?;
			assert!(!fvar.axes.iter().any(|a| a.tag_text() == axis));
			for instance in &fvar.instances {
				assert!(!instance.coordinates.contains_key(&tag(axis)));
			}
			let ids: Vec<u16> = fvar.instances.iter().map(|i| i.subfamily_name_id).collect();
			let name = font.require::<Name>()?;
			let names: Vec<String> = ids
				.iter()
				.map(|id| name.get_debug_name(*id).unwrap_or_default())
				.collect();
			assert_eq!(names, instances_left);
		}
		Ok(())
	}

	#[test]
	fn test_full_instance() -> TestResult {
		let mut font = varfont()?;
		let limits = user_limits(&mut font, &[("wght", pin(0.0)), ("wdth", pin(0.0))])?;
		instantiate_fvar(&mut font, &limits)?;
		assert!(!font.contains(Tag::new(b"fvar")));
		Ok(())
	}

	#[test]
	fn test_axis_limits() -> TestResult {
		let n = |v: f64| Some(v);
		let cases = [
			((n(30.0), n(40.0), n(700.0)), (100.0, 100.0, 700.0)),
			((n(30.0), n(40.0), None), (100.0, 100.0, 900.0)),
			((n(30.0), None, n(700.0)), (100.0, 400.0, 700.0)),
			((None, n(200.0), n(700.0)), (100.0, 200.0, 700.0)),
			((n(40.0), None, None), (100.0, 400.0, 900.0)),
			((None, n(40.0), None), (100.0, 100.0, 900.0)),
			((None, None, n(700.0)), (100.0, 400.0, 700.0)),
			((None, None, None), (100.0, 400.0, 900.0)),
		];
		for ((minimum, default, maximum), expected) in cases {
			let font = varfont()?;
			let triple = AxisTriple::new(minimum, default, maximum)?;
			let mut font = instantiated(&font, &[("wght", Some(triple))])?;

			let fvar = font.require::<Fvar>()?;
			let axis = fvar
				.axes
				.iter()
				.find(|a| a.tag_text() == "wght")
				.ok_or("no wght")?;
			assert_eq!(
				(axis.min_value, axis.default_value, axis.max_value),
				expected
			);
		}
		Ok(())
	}
}

mod instantiate_stat_test {
	use std::collections::BTreeSet;

	use fontoxide_ttlib::Font;
	use fontoxide_ttlib::ot::types as ot;
	use fontoxide_ttlib::tables::{Name, Stat};
	use fontoxide_varlib::instancer::instantiate_stat;

	use super::*;

	#[test]
	fn test_pin_and_drop_axis() -> TestResult {
		let cases: [(&str, f64, &[&str]); 2] = [
			(
				"wght",
				400.0,
				&["Regular", "Condensed", "Upright", "Normal"],
			),
			(
				"wdth",
				100.0,
				&["Thin", "Regular", "Medium", "Black", "Upright", "Normal"],
			),
		];
		for (axis, value, expected) in cases {
			let mut font = varfont()?;
			let limits = user_limits(&mut font, &[(axis, pin(value))])?;

			instantiate_stat(&mut font, &limits)?;

			let stat = font.require::<Stat>()?;
			let design_axes: BTreeSet<String> = stat
				.design_axis_record
				.as_ref()
				.ok_or("no axes")?
				.axis
				.iter()
				.map(|a| a.axis_tag.to_string())
				.collect();
			assert_eq!(
				design_axes,
				["wght", "wdth", "ital"]
					.map(str::to_owned)
					.into_iter()
					.collect()
			);
			let ids: Vec<u16> = stat
				.axis_value_array
				.as_ref()
				.ok_or("no values")?
				.axis_value
				.iter()
				.flatten()
				.map(|v| match &**v {
					ot::AxisValue::Format1(v) => v.value_name_id,
					ot::AxisValue::Format2(v) => v.value_name_id,
					ot::AxisValue::Format3(v) => v.value_name_id,
					ot::AxisValue::Format4(v) => v.value_name_id,
					ot::AxisValue::Unknown(_) => 0,
				})
				.collect();
			let name = font.require::<Name>()?;
			let names: Vec<String> = ids
				.iter()
				.map(|id| name.get_debug_name(*id).unwrap_or_default())
				.collect();
			assert_eq!(names, expected);
		}
		Ok(())
	}

	fn design_axis_count(font: &mut Font) -> TestResult<usize> {
		Ok(font
			.require::<Stat>()?
			.design_axis_record
			.as_ref()
			.ok_or("no axes")?
			.axis
			.len())
	}

	#[test]
	fn test_skip_table_no_axis_value_array() -> TestResult {
		let mut font = varfont()?;
		font.require::<Stat>()?.axis_value_array = None;
		let limits = user_limits(&mut font, &[("wght", pin(100.0))])?;

		instantiate_stat(&mut font, &limits)?;

		assert_eq!(design_axis_count(&mut font)?, 3);
		assert!(font.require::<Stat>()?.axis_value_array.is_none());
		Ok(())
	}

	#[test]
	fn test_skip_table_axis_value_array_empty() -> TestResult {
		let mut font = varfont()?;
		font.require::<Stat>()?
			.axis_value_array
			.as_mut()
			.ok_or("no values")?
			.axis_value
			.clear();
		let limits = user_limits(&mut font, &[("wght", pin(100.0))])?;

		instantiate_stat(&mut font, &limits)?;

		assert_eq!(design_axis_count(&mut font)?, 3);
		let stat = font.require::<Stat>()?;
		assert!(
			stat.axis_value_array
				.as_ref()
				.ok_or("no values")?
				.axis_value
				.is_empty()
		);
		Ok(())
	}

	#[test]
	fn test_skip_table_no_design_axes() -> TestResult {
		let mut font = varfont()?;
		let limits = user_limits(&mut font, &[("wght", pin(100.0))])?;
		let stat = font.require::<Stat>()?;
		stat.table = ot::STAT {
			version: 0x0001_0001,
			..Default::default()
		};
		assert!(stat.design_axis_record.is_none());
		assert!(stat.axis_value_array.is_none());

		instantiate_stat(&mut font, &limits)?;

		assert!(font.require::<Stat>()?.design_axis_record.is_none());
		Ok(())
	}

	#[derive(Debug, PartialEq)]
	enum Value {
		Single(String, f64),
		Linked(String, f64, f64),
		Range(String, f64, f64, f64),
	}

	fn get_stat_axis_values(stat: &Stat) -> TestResult<Vec<Value>> {
		let axes = &stat.design_axis_record.as_ref().ok_or("no axes")?.axis;
		let name = |i: u16| axes[usize::from(i)].axis_tag.to_string();
		let mut result = Vec::new();
		let values = &stat
			.axis_value_array
			.as_ref()
			.ok_or("no values")?
			.axis_value;
		for value in values.iter().flatten() {
			result.push(match &**value {
				ot::AxisValue::Format1(v) => Value::Single(name(v.axis_index), v.value),
				ot::AxisValue::Format3(v) => {
					Value::Linked(name(v.axis_index), v.value, v.linked_value)
				}
				ot::AxisValue::Format2(v) => Value::Range(
					name(v.axis_index),
					v.range_min_value,
					v.nominal_value,
					v.range_max_value,
				),
				_ => return Err("unexpected format".into()),
			});
		}
		Ok(result)
	}

	#[test]
	fn test_limit_axes() -> TestResult {
		let mut font = varfont2()?;
		let limits = user_limits(
			&mut font,
			&[("wght", range(400.0, 500.0)), ("wdth", range(75.0, 100.0))],
		)?;

		instantiate_stat(&mut font, &limits)?;

		let stat = font.require::<Stat>()?;
		let count = stat
			.axis_value_array
			.as_ref()
			.ok_or("no values")?
			.axis_value
			.len();
		assert_eq!(count, 5);
		assert_eq!(
			get_stat_axis_values(stat)?,
			[
				Value::Linked("wght".into(), 400.0, 700.0),
				Value::Single("wght".into(), 500.0),
				Value::Range("wdth".into(), 93.75, 100.0, 100.0),
				Value::Range("wdth".into(), 81.25, 87.5, 93.75),
				Value::Range("wdth".into(), 68.75, 75.0, 81.25),
			]
		);
		Ok(())
	}

	fn has_value(font: &mut Font, value: &ot::AxisValue) -> TestResult<bool> {
		Ok(font
			.require::<Stat>()?
			.axis_value_array
			.as_ref()
			.ok_or("no values")?
			.axis_value
			.iter()
			.flatten()
			.any(|v| **v == *value))
	}

	#[test]
	fn test_limit_axis_value_format_4() -> TestResult {
		let mut font = varfont2()?;
		let stat = font.require::<Stat>()?;
		let axes = &stat.design_axis_record.as_ref().ok_or("no axes")?.axis;
		let index = |tag: &str| {
			axes.iter()
				.position(|a| a.axis_tag == tag)
				.map(|i| i as u16)
				.ok_or("no axis")
		};
		let axis_value = ot::AxisValue::Format4(ot::AxisValueFormat4 {
			axis_value_record: vec![
				ot::AxisValueRecord {
					axis_index: index("wght")?,
					value: 575.0,
				},
				ot::AxisValueRecord {
					axis_index: index("wdth")?,
					value: 90.0,
				},
			],
			..Default::default()
		});
		stat.axis_value_array
			.as_mut()
			.ok_or("no values")?
			.axis_value
			.push(Some(Box::new(axis_value.clone())));

		let limits = user_limits(&mut font, &[("wght", range(100.0, 600.0))])?;
		instantiate_stat(&mut font, &limits)?;

		assert!(has_value(&mut font, &axis_value)?);

		let limits = user_limits(&mut font, &[("wdth", range(62.5, 87.5))])?;
		instantiate_stat(&mut font, &limits)?;

		assert!(!has_value(&mut font, &axis_value)?);
		Ok(())
	}

	#[test]
	fn test_unknown_axis_value_format() -> TestResult {
		let mut font = varfont2()?;
		let axis_value = ot::AxisValue::Unknown(5);
		font.require::<Stat>()?
			.axis_value_array
			.as_mut()
			.ok_or("no values")?
			.axis_value
			.push(Some(Box::new(axis_value.clone())));
		let limits = user_limits(&mut font, &[("wght", pin(400.0))])?;

		let (text, result) = capture_warnings(|| instantiate_stat(&mut font, &limits));
		result?;

		assert!(
			text.contains("Unknown AxisValue table format (5)"),
			"{text}"
		);
		assert!(has_value(&mut font, &axis_value)?);
		Ok(())
	}
}

#[test]
fn test_set_mac_overlap_flags() -> TestResult {
	use fontoxide_ttlib::Font;
	use fontoxide_ttlib::tables::Glyf;
	use fontoxide_ttlib::tables::glyf::{ComponentPlacement, Glyph, GlyphComponent, Number};
	use fontoxide_varlib::instancer::set_mac_overlap_flags;

	const FLAG_OVERLAP_COMPOUND: i64 = 0x0400;
	const FLAG_OVERLAP_SIMPLE: u8 = 0x40;

	let mut font = Font::new();
	font.set_glyph_order(vec!["a".to_owned(), "b".to_owned(), "c".to_owned()]);
	let mut glyf = Glyf::default();
	glyf.glyphs.insert(
		0,
		Glyph {
			number_of_contours: 1,
			flags: vec![0],
			..Default::default()
		},
	);
	glyf.glyphs.insert(
		1,
		Glyph {
			number_of_contours: -1,
			components: vec![GlyphComponent {
				glyph: 0,
				flags: 0,
				placement: ComponentPlacement::Offset {
					x: Number::Int(0),
					y: Number::Int(0),
				},
				transform: None,
			}],
			..Default::default()
		},
	);
	glyf.glyphs.insert(2, Glyph::default());
	font.insert(Box::new(glyf));

	set_mac_overlap_flags(&mut font)?;

	let glyf = font.require::<Glyf>()?;
	assert_ne!(glyf.glyphs[&0].flags[0] & FLAG_OVERLAP_SIMPLE, 0);
	assert_ne!(
		glyf.glyphs[&1].components[0].flags & FLAG_OVERLAP_COMPOUND,
		0
	);
	Ok(())
}
