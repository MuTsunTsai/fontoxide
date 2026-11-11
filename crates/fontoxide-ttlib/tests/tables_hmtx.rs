//! `Tests/ttLib/tables/_h_m_t_x_test.py` and `Tests/ttLib/tables/_v_m_t_x_test.py`
//! (which runs the same tests on `vmtx`).

#![allow(clippy::unwrap_used)]

mod common;

macro_rules! metrics_tests {
	($module:ident, $ty:ident, $header:ident, $field:ident, $tag:literal, $header_tag:literal, $number_name:literal, $advance:literal, $side_bearing:literal) => {
		mod $module {
			use std::collections::BTreeMap;

			use crate::common::*;
			use fontoxide_ttlib::tables::{Maxp, $header, $ty};
			use fontoxide_ttlib::{Font, Table, TableType};

			/// `makeFont(numGlyphs, numberOfMetrics)`.
			fn make_font(num_glyphs: u32, number_of_metrics: u32) -> Font {
				let mut font = Font::new();
				font.insert(Box::new(Maxp {
					num_glyphs,
					..Default::default()
				}));
				font.set_glyph_order(
					(0..num_glyphs)
						.map(|i| char::from(65 + i as u8).to_string())
						.collect(),
				);
				font.insert(Box::new($header {
					$field: number_of_metrics,
					..Default::default()
				}));
				font
			}

			fn metrics(list: &[(u32, (u16, i16))]) -> BTreeMap<u32, (u16, i16)> {
				list.iter().copied().collect()
			}

			#[test]
			fn test_decompile() -> TestResult {
				let mut font = make_font(3, 3);
				let table = $ty::decompile(&hex("02A2 FFF5 0278 004F 02C6 0036"), &mut font)?;
				assert_eq!(table.get(0), Some((674, -11)));
				assert_eq!(table.get(1), Some((632, 79)));
				assert_eq!(table.get(2), Some((710, 54)));
				Ok(())
			}

			#[test]
			fn test_decompile_additional_sb() -> TestResult {
				let mut font = make_font(4, 2);
				let table = $ty::decompile(&hex("02A2 FFF5 0278 004F 0036 FFFC"), &mut font)?;
				assert_eq!(table.get(0), Some((674, -11)));
				assert_eq!(table.get(1), Some((632, 79)));
				// All following have the same advance as the previous.
				assert_eq!(table.get(2), Some((632, 54)));
				assert_eq!(table.get(3), Some((632, -4)));
				Ok(())
			}

			#[test]
			fn test_decompile_not_enough_data_for_long_metrics() {
				let mut font = make_font(1, 1);
				let error = $ty::decompile(b"\0\0\0", &mut font).unwrap_err();
				assert!(
					error
						.to_string()
						.contains(concat!("not enough '", $tag, "' table data")),
					"{error}"
				);
			}

			#[test]
			fn test_decompile_not_enough_data_for_sidebearings() {
				let mut font = make_font(2, 1);
				let error = $ty::decompile(b"\0\0\0\0", &mut font).unwrap_err();
				assert!(
					error.to_string().contains(concat!(
						"not enough '",
						$tag,
						"' table data: expected 6 bytes, got 4"
					)),
					"{error}"
				);
			}

			#[test]
			fn test_decompile_too_much_data() -> TestResult {
				let mut font = make_font(1, 1);
				let (logs, table) = capture_logs(|| $ty::decompile(b"\0\0\0\0\0", &mut font));
				table?;
				let message = concat!("too much '", $tag, "' table data");
				assert_eq!(
					logs.iter()
						.filter(|(l, m)| *l == log::Level::Warn && m == message)
						.count(),
					1
				);
				Ok(())
			}

			#[test]
			fn test_decompile_num_metrics_greater_than_glyphs() -> TestResult {
				let mut font = make_font(1, 2);
				let (logs, table) = capture_logs(|| $ty::decompile(b"\0\0\0\0", &mut font));
				table?;
				let message = concat!(
					"The ",
					$header_tag,
					".",
					$number_name,
					" exceeds the maxp.numGlyphs"
				);
				assert_eq!(
					logs.iter()
						.filter(|(l, m)| *l == log::Level::Warn && m == message)
						.count(),
					1
				);
				Ok(())
			}

			#[test]
			fn test_decompile_possibly_negative_advance() -> TestResult {
				let mut font = make_font(1, 1);
				let (logs, table) = capture_logs(|| $ty::decompile(&hex("8000 0000"), &mut font));
				table?;
				assert_eq!(count_logs(&logs, log::Level::Warn, "has a huge advance"), 1);
				Ok(())
			}

			#[test]
			fn test_decompile_no_header_table() -> TestResult {
				let mut font = Font::new();
				font.insert(Box::new(Maxp {
					num_glyphs: 3,
					..Default::default()
				}));
				font.set_glyph_order(names(&["A", "B", "C"]));
				assert!(!font.has::<$header>());
				let table = $ty::decompile(&hex("0190 001E 0190 0028 0190 0032"), &mut font)?;
				assert_eq!(
					table.metrics,
					metrics(&[(0, (400, 30)), (1, (400, 40)), (2, (400, 50))])
				);
				Ok(())
			}

			#[test]
			fn test_compile() -> TestResult {
				// The wrong numberOfMetrics is adjusted.
				let mut font = make_font(3, 4);
				let mut table = $ty::default();
				table.metrics = metrics(&[(0, (674, -11)), (1, (632, 79)), (2, (710, 54))]);
				assert_eq!(
					table.compile(&mut font)?,
					hex("02A2 FFF5 0278 004F 02C6 0036")
				);
				assert_eq!(font.require::<$header>()?.$field, 3);
				Ok(())
			}

			#[test]
			fn test_compile_additional_sb() -> TestResult {
				let mut font = make_font(4, 1);
				let mut table = $ty::default();
				table.metrics = metrics(&[
					(0, (632, -11)),
					(1, (632, 79)),
					(2, (632, 54)),
					(3, (632, -4)),
				]);
				assert_eq!(table.compile(&mut font)?, hex("0278 FFF5 004F 0036 FFFC"));
				Ok(())
			}

			#[test]
			fn test_compile_no_header_table() -> TestResult {
				let mut font = Font::new();
				font.insert(Box::new(Maxp {
					num_glyphs: 3,
					..Default::default()
				}));
				font.set_glyph_order(names(&["A", "B", "C"]));
				let mut table = $ty::default();
				table.metrics = metrics(&[(0, (400, 30)), (1, (400, 40)), (2, (400, 50))]);
				assert!(!font.has::<$header>());
				assert_eq!(
					table.compile(&mut font)?,
					hex("0190 001E 0190 0028 0190 0032")
				);
				Ok(())
			}

			#[test]
			fn test_to_xml() -> TestResult {
				let mut font = make_font(2, 2);
				let mut table = $ty::default();
				table.metrics = metrics(&[(1, (632, 79)), (0, (674, -11))]);
				assert_eq!(
					get_xml(&table, &mut font)?,
					[
						concat!(
							r#"<mtx name="A" "#,
							$advance,
							r#"="674" "#,
							$side_bearing,
							r#"="-11"/>"#
						),
						concat!(
							r#"<mtx name="B" "#,
							$advance,
							r#"="632" "#,
							$side_bearing,
							r#"="79"/>"#
						),
					]
				);
				Ok(())
			}

			#[test]
			fn test_from_xml() -> TestResult {
				let mut font = fake_font(&["A", "B"]);
				let mut table = $ty::default();
				from_xml(
					&mut table,
					concat!(
						r#"<mtx name="A" "#,
						$advance,
						r#"="674" "#,
						$side_bearing,
						r#"="-11"/>"#,
						r#"<mtx name="B" "#,
						$advance,
						r#"="632" "#,
						$side_bearing,
						r#"="79"/>"#,
					),
					&mut font,
				)?;
				assert_eq!(table.metrics, metrics(&[(0, (674, -11)), (1, (632, 79))]));
				Ok(())
			}

			#[test]
			fn test_delitem() {
				let mut table = $ty::default();
				table.metrics = metrics(&[(0, (0, 0))]);
				table.metrics.remove(&0);
				assert!(!table.metrics.contains_key(&0));
			}

			#[test]
			fn test_setitem() {
				let mut table = $ty::default();
				table.metrics = metrics(&[(0, (674, -11)), (1, (632, 79))]);
				table.set(1, (0, 0));
				assert_eq!(table.metrics, metrics(&[(0, (674, -11)), (1, (0, 0))]));
			}
		}
	};
}

metrics_tests!(
	hmtx,
	Hmtx,
	Hhea,
	number_of_h_metrics,
	"hmtx",
	"hhea",
	"numberOfHMetrics",
	"width",
	"lsb"
);
metrics_tests!(
	vmtx,
	Vmtx,
	Vhea,
	number_of_v_metrics,
	"vmtx",
	"vhea",
	"numberOfVMetrics",
	"height",
	"tsb"
);
