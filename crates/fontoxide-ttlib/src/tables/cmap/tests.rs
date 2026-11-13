//! Ports of `Tests/ttLib/tables/_c_m_a_p_test.py`.

use super::*;
use crate::{ReadOptions, TableKey, TtxOptions};

/// A (code, glyph ID) pair.
type Pair = (u32, u32);

/// A format 12/13 group: (start code, end code, glyph ID).
type Group = (u32, u32, u32);

fn font_with(names: Vec<String>) -> Font {
	let mut font = Font::new();
	font.set_glyph_order(names);
	font
}

fn names(list: &[&str]) -> Vec<String> {
	list.iter().map(|s| (*s).to_owned()).collect()
}

fn make_subtable(format: u16, platform_id: u16, plat_enc_id: u16, language: u32) -> CmapSubtable {
	let mut t = CmapSubtable::new(format);
	t.platform_id = platform_id;
	t.plat_enc_id = plat_enc_id;
	t.language = Some(language);
	t
}

fn charmap(pairs: &[(u32, u32)]) -> Arc<CharMap> {
	Arc::new(pairs.iter().copied().collect())
}

fn reload(format: u16, data: &[u8]) -> CmapSubtable {
	let mut t = CmapSubtable::new(format);
	match t.decompile(data) {
		Ok(()) => t,
		Err(e) => panic!("decompile failed: {e}"),
	}
}

fn compile_ok(t: &CmapSubtable, font: &mut Font) -> Vec<u8> {
	match t.compile(font) {
		Ok(d) => d,
		Err(e) => panic!("compile failed: {e}"),
	}
}

fn upstream_data(path: &str) -> std::path::PathBuf {
	std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("../../upstream/fonttools/Tests/ttLib/tables/data")
		.join(path)
}

#[test]
fn is_unicode() {
	assert!(make_subtable(4, 0, 2, 7).is_unicode());
	assert!(!make_subtable(4, 1, 0, 7).is_unicode());
	assert!(!make_subtable(4, 1, 0, 37).is_unicode());
	assert!(!make_subtable(4, 1, 1, 0).is_unicode());
	assert!(make_subtable(4, 3, 0, 0).is_symbol());
	assert!(make_subtable(4, 3, 10, 0).is_unicode());
}

#[test]
fn compact_glyph_id_limits() {
	let mut order = vec![".notdef".to_owned()];
	order.extend((1..0x10002).map(|i| format!("g{i}")));
	let mut font = font_with(order);
	for format in [2, 4] {
		for companion in [false, true] {
			for gid in [0xFFFF, 0x10000, 0x10001] {
				let mut t = make_subtable(format, 3, 1, 0);
				let mut pairs = vec![(65, gid)];
				if companion {
					pairs.push((66, 2));
				}
				t.cmap = charmap(&pairs);
				if gid > 0xFFFF {
					match t.compile(&mut font) {
						Err(Error::Struct(m)) => assert!(m.contains("glyph ID"), "{m}"),
						other => panic!("expected struct error, got {other:?}"),
					}
				} else {
					let data = compile_ok(&t, &mut font);
					assert_eq!(reload(format, &data).cmap, t.cmap);
				}
			}
		}
	}
	// Virtual glyph IDs ("gidNNN" names) read from TTX.
	for format in [2u16, 4] {
		for gid in [0xFFFFu32, 0x10000] {
			let ttx = format!(
				"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ttFont>\n<cmap>\n<tableVersion version=\"0\"/>\n\
				 <cmap_format_{format} platformID=\"3\" platEncID=\"1\" language=\"0\">\n\
				 <map code=\"0x41\" name=\"gid{gid}\"/>\n</cmap_format_{format}>\n</cmap>\n</ttFont>\n"
			);
			let mut font = font_with(names(&[".notdef"]));
			if let Err(e) = font.import_ttx(&ttx) {
				panic!("{e}");
			}
			let result = font.table_data(Cmap::TAG);
			if gid > 0xFFFF {
				assert!(result.is_err());
			} else {
				let data = result.unwrap_or_default();
				let table = Cmap::parse(&data).unwrap_or_default();
				assert_eq!(
					*table.tables[0].cmap,
					[(65, gid)].into_iter().collect::<CharMap>()
				);
			}
		}
	}
}

fn directory(num_tables: u16, entries: &[(u16, u16, u32)], body: &[u8]) -> Vec<u8> {
	let mut data = Vec::new();
	data.put_u16(0);
	data.put_u16(num_tables);
	for &(p, e, o) in entries {
		data.put_u16(p);
		data.put_u16(e);
		data.put_u32(o);
	}
	data.extend_from_slice(body);
	data
}

#[test]
fn decompile_truncated() {
	let one_subtable = |header: &[u8]| directory(1, &[(3, 1, 12)], header);
	let mut length_4 = Vec::new();
	length_4.put_u16(4);
	length_4.put_u16(4);
	let mut length_20 = Vec::new();
	length_20.put_u16(4);
	length_20.put_u16(20);
	length_20.put_u16(0);
	let mut short_dir = directory(3, &[], &[]);
	short_dir.extend_from_slice(b"ab");
	let cases: Vec<(Vec<u8>, &str)> = vec![
		(b"\x00\x00".to_vec(), "too short"),
		(short_dir, "directory is truncated"),
		(directory(1, &[(3, 1, 9999)], &[]), "out of bounds"),
		(directory(1, &[(3, 1, 0xFFFFFFFF)], &[]), "out of bounds"),
		(one_subtable(&length_4), "header is truncated"),
		(one_subtable(&length_20), "subtable is truncated"),
	];
	for (data, pattern) in cases {
		match Cmap::parse(&data) {
			Err(Error::TtLib(m)) => assert!(m.contains(pattern), "{m} !~ {pattern}"),
			other => panic!("expected TTLibError for {pattern}, got {other:?}"),
		}
	}
}

#[test]
fn decompile_unknown_format_no_header_minimum() {
	for (format, header) in [
		(8u16, vec![0, 8, 0, 0, 0, 0, 0, 4]),
		(10, vec![0, 10, 0, 0, 0, 0, 0, 4]),
		(99, vec![0, 99, 0, 4]),
	] {
		let data = directory(1, &[(3, 1, 12)], &header);
		let table = match Cmap::parse(&data) {
			Ok(t) => t,
			Err(e) => panic!("{e}"),
		};
		let subtable = &table.tables[0];
		assert_eq!(subtable.kind, SubtableKind::Unknown);
		assert_eq!(subtable.format, format);
		assert_eq!(subtable.data.as_deref(), Some(&header[..4]));
	}
}

fn header_12(format: u16, length: u32, n_groups: u32) -> Vec<u8> {
	let mut d = Vec::new();
	d.put_u16(format);
	d.put_u16(0);
	d.put_u32(length);
	d.put_u32(0);
	d.put_u32(n_groups);
	d
}

#[test]
fn decompile_format_12_inconsistent_n_groups() {
	let mut body = header_12(12, 28, 0);
	body.extend([0; 12]);
	let data = directory(1, &[(3, 10, 12)], &body);
	match Cmap::parse(&data) {
		Err(Error::TtLib(m)) => assert!(m.contains("inconsistent group count"), "{m}"),
		other => panic!("{other:?}"),
	}
}

#[test]
fn decompile_subtable_format_12_truncated() {
	let mut t = CmapSubtable::new(12);
	match t.decompile(&header_12(12, 28, 1)) {
		Err(Error::TtLib(m)) => assert!(m.contains("is truncated"), "{m}"),
		other => panic!("{other:?}"),
	}
}

#[test]
fn compile_2() {
	let mut t = make_subtable(2, 1, 2, 0);
	t.cmap = Arc::new((32..8192).map(|c| (c, c - 31)).collect());
	let mut order = vec![".notdef".to_owned()];
	order.extend((32..8192).map(|c| format!("cid{c:05}")));
	let mut font = font_with(order);
	let data = compile_ok(&t, &mut font);
	assert_eq!(reload(2, &data).cmap, t.cmap);
}

#[test]
fn compile_2_rebuild_rev_glyph_order() {
	for format in [2, 4, 12] {
		let mut t = make_subtable(format, 1, 2, 0);
		t.cmap = Arc::new((32..8192).map(|c| (c, c - 31)).collect());
		let mut order = vec![".notdef".to_owned()];
		order.extend((32..8192).map(|c| format!("cid{c:05}")));
		let mut font = font_with(order);
		let data = compile_ok(&t, &mut font);
		assert_eq!(reload(format, &data).cmap, t.cmap, "{format}");
	}
}

#[test]
fn compile_2_gids() {
	for format in [2, 4, 12] {
		let ttx = format!(
			"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ttFont>\n<cmap>\n<tableVersion version=\"0\"/>\n\
			 <cmap_format_{format} platformID=\"1\" platEncID=\"3\" language=\"0\" format=\"{format}\" \
			 reserved=\"0\" length=\"0\" nGroups=\"0\">\n\
			 <map code=\"0x41\" name=\"gid001\"/>\n<map code=\"0x42\" name=\"gid002\"/>\n\
			 </cmap_format_{format}>\n</cmap>\n</ttFont>\n"
		);
		let mut font = font_with(names(&[".notdef"]));
		if let Err(e) = font.import_ttx(&ttx) {
			panic!("{e}");
		}
		let data = match font.table_data(Cmap::TAG) {
			Ok(d) => d,
			Err(e) => panic!("{format}: {e}"),
		};
		let table = Cmap::parse(&data).unwrap_or_default();
		assert_eq!(
			*table.tables[0].cmap,
			[(0x41, 1), (0x42, 2)].into_iter().collect::<CharMap>()
		);
	}
}

#[test]
fn compile_2_high_gids() {
	let mut order = vec![".notdef".to_owned()];
	order.extend((1..65536).map(|i| format!("glyph{i:05}")));
	let mut font = font_with(order);
	for gids in [
		&[32767u32][..],
		&[32768],
		&[32769],
		&[40000, 40001],
		&[65535],
	] {
		let mut t = make_subtable(2, 1, 1, 0);
		t.cmap = Arc::new(
			gids.iter()
				.enumerate()
				.map(|(i, &g)| (0x8140 + i as u32, g))
				.collect(),
		);
		let data = compile_ok(&t, &mut font);
		assert_eq!(reload(2, &data).cmap, t.cmap, "{gids:?}");
	}
}

#[test]
fn compile_2_single_byte_only() {
	let mut font = font_with(names(&[".notdef", "a", "b"]));
	for pairs in [
		&[(0x20u32, 1u32)][..],
		&[(0x41, 1), (0x42, 2)],
		&[(0xFF, 2)],
	] {
		let mut t = make_subtable(2, 1, 1, 0);
		t.cmap = charmap(pairs);
		let data = compile_ok(&t, &mut font);
		assert_eq!(reload(2, &data).cmap, t.cmap, "{pairs:?}");
	}
}

#[test]
fn compile_2_single_byte_notdef() {
	let mut font = font_with(names(&[".notdef", "a", "b"]));
	let cases: [(&[Pair], &[Pair]); 3] = [
		(&[(0x41, 0)], &[]),
		(&[(0x41, 0), (0x8140, 1)], &[(0x8140, 1)]),
		(
			&[(0x00, 1), (0x41, 0), (0x8140, 2)],
			&[(0x00, 1), (0x8140, 2)],
		),
	];
	for (pairs, expected) in cases {
		let mut t = make_subtable(2, 1, 1, 0);
		t.cmap = charmap(pairs);
		let data = compile_ok(&t, &mut font);
		assert_eq!(reload(2, &data).cmap, charmap(expected), "{pairs:?}");
	}
}

#[test]
fn compile_decompile_4_empty() {
	let mut font = font_with(Vec::new());
	let t = make_subtable(4, 3, 1, 0);
	let data = compile_ok(&t, &mut font);
	assert!(reload(4, &data).cmap.is_empty());
}

#[test]
fn compile_decompile_12_13_empty() {
	let mut font = font_with(Vec::new());
	for format in [12, 13] {
		let t = make_subtable(format, 3, 10, 0);
		let data = compile_ok(&t, &mut font);
		assert_eq!(data.len(), 16);
		let t2 = reload(format, &data);
		assert!(t2.cmap.is_empty());
		assert_eq!(compile_ok(&t2, &mut font), data);
	}
}

#[test]
fn compile_decompile_2_empty() {
	let mut font = font_with(Vec::new());
	let t = make_subtable(2, 1, 3, 0);
	let data = compile_ok(&t, &mut font);
	assert!(reload(2, &data).cmap.is_empty());
}

#[test]
fn decompile_compile_2_all_notdef() {
	let mut font = font_with(names(&[".notdef", "A", "B"]));
	let mut body = vec![0u8; 512];
	body.extend([0u8; 8]);
	let mut data = Vec::new();
	data.put_u16(2);
	data.put_u16(6 + body.len() as u16);
	data.put_u16(0);
	data.extend(body);
	let mut t = CmapSubtable::new(2);
	t.platform_id = 1;
	t.plat_enc_id = 3;
	if let Err(e) = t.decompile(&data) {
		panic!("{e}");
	}
	assert!(t.cmap.is_empty());
	let t2 = reload(2, &compile_ok(&t, &mut font));
	assert!(t2.cmap.is_empty());
}

#[test]
fn decompile_4() {
	let mut data = vec![0u8; 3];
	data.push(0x10);
	data.extend([0u8; 12]);
	reload(4, &data);
}

#[test]
fn decompile_12() {
	let mut data = vec![0u8; 7];
	data.push(0x10);
	data.extend([0u8; 8]);
	reload(12, &data);
}

fn decompile_12_or_13(format: u16, groups: &[(u32, u32, u32)]) -> CharMap {
	let mut body = Vec::new();
	for &(s, e, g) in groups {
		body.put_u32(s);
		body.put_u32(e);
		body.put_u32(g);
	}
	let mut data = header_12(format, 16 + body.len() as u32, groups.len() as u32);
	data.extend(body);
	(*reload(format, &data).cmap).clone()
}

fn map(pairs: &[(u32, u32)]) -> CharMap {
	pairs.iter().copied().collect()
}

#[test]
fn decompile_12_or_13_clamps_to_max_unicode() {
	for format in [12, 13] {
		let cmap = decompile_12_or_13(format, &[(0x10FFFE, 0xFFFFFFFF, 1)]);
		let second = if format == 12 { 2 } else { 1 };
		assert_eq!(cmap, map(&[(0x10FFFE, 1), (0x10FFFF, second)]));
	}
}

#[test]
fn decompile_12_or_13_many_overlapping_groups() {
	for format in [12, 13] {
		let cmap = decompile_12_or_13(format, &[(0, 0xFFFFFFFF, 1); 1000]);
		assert_eq!(cmap.len(), 0x110000);
	}
}

#[test]
fn decompile_12_or_13_bad_groups_skipped() {
	let cases: [(u16, &[Group], &[Pair]); 4] = [
		(
			12,
			&[(0x41, 0x43, 1), (0x42, 0x44, 2)],
			&[(0x41, 1), (0x42, 2), (0x43, 3)],
		),
		(12, &[(0x43, 0x43, 3), (0x41, 0x42, 1)], &[(0x43, 3)]),
		(12, &[(0x42, 0x41, 1), (0x200000, 0x200001, 1)], &[]),
		(13, &[(0x42, 0x41, 1), (0x200000, 0x200001, 1)], &[]),
	];
	for (format, groups, expected) in cases {
		assert_eq!(
			decompile_12_or_13(format, groups),
			map(expected),
			"{groups:?}"
		);
	}
}

#[test]
fn decompile_12_or_13_shared_endpoint() {
	let cases: [(u16, &[Group], &[Pair]); 4] = [
		(
			12,
			&[(0x41, 0x42, 2), (0x42, 0x43, 1)],
			&[(0x41, 2), (0x42, 1), (0x43, 2)],
		),
		(
			13,
			&[(0x41, 0x42, 1), (0x42, 0x43, 2)],
			&[(0x41, 1), (0x42, 2), (0x43, 2)],
		),
		(
			12,
			&[(0x41, 0x42, 1), (0x42, 0x43, 0)],
			&[(0x41, 1), (0x42, 2), (0x43, 1)],
		),
		(
			13,
			&[(0x41, 0x42, 1), (0x42, 0x43, 0)],
			&[(0x41, 1), (0x42, 1)],
		),
	];
	for (format, groups, expected) in cases {
		assert_eq!(
			decompile_12_or_13(format, groups),
			map(expected),
			"{groups:?}"
		);
	}
}

#[test]
fn decompile_13_full_range_missing_glyph() {
	assert!(decompile_12_or_13(13, &[(0, 0x10FFFF, 0)]).is_empty());
}

#[test]
fn decompile_12_starting_missing_glyph() {
	assert_eq!(
		decompile_12_or_13(12, &[(0x41, 0x43, 0)]),
		map(&[(0x42, 1), (0x43, 2)])
	);
}

#[test]
fn decompile_12_sorted_groups_no_warning() {
	let cmap = decompile_12_or_13(
		12,
		&[(0x41, 0x42, 1), (0x43, 0x43, 3), (0x10FFFF, 0x10FFFF, 1)],
	);
	assert_eq!(cmap, map(&[(0x41, 1), (0x42, 2), (0x43, 3), (0x10FFFF, 1)]));
}

/// Segments are `(start, end, delta, rangeOffset)`; the 0xFFFF sentinel segment is
/// appended.
fn decompile_4_segments(
	segments: &[(u16, u16, i32, u16)],
	glyph_index_array: &[u16],
) -> Result<CharMap> {
	let mut segments = segments.to_vec();
	segments.push((0xFFFF, 0xFFFF, 1, 0));
	let n = segments.len() as u16;
	let mut body = Vec::new();
	for v in [n * 2, 0, 0, 0] {
		body.put_u16(v);
	}
	for s in &segments {
		body.put_u16(s.1);
	}
	body.put_u16(0);
	for s in &segments {
		body.put_u16(s.0);
	}
	for s in &segments {
		body.put_u16((s.2 & 0xFFFF) as u16);
	}
	for s in &segments {
		body.put_u16(s.3);
	}
	for &g in glyph_index_array {
		body.put_u16(g);
	}
	let mut data = Vec::new();
	data.put_u16(4);
	data.put_u16(6 + body.len() as u16);
	data.put_u16(0);
	data.extend(body);
	let mut t = CmapSubtable::new(4);
	t.decompile(&data)?;
	Ok((*t.cmap).clone())
}

#[test]
fn decompile_4_overlapping_segments() {
	let cmap = decompile_4_segments(&[(0, 0xFFFE, 1, 0); 8], &[]).unwrap_or_default();
	assert_eq!(cmap.len(), 0xFFFF);
}

#[test]
fn decompile_4_later_segment_wins() {
	let cmap = decompile_4_segments(
		&[(0x41, 0x43, 0, 6), (0x42, 0x43, 0, 4 + 6)],
		&[1, 2, 3, 4, 0],
	);
	assert_eq!(cmap.ok(), Some(map(&[(0x41, 1), (0x42, 4), (0x43, 3)])));
}

#[test]
fn decompile_4_id_range_offset_out_of_bounds() {
	for range_offset in [2, 0xFFF0] {
		let result = decompile_4_segments(&[(0x41, 0x41, 0, range_offset)], &[1, 2, 3]);
		assert!(
			matches!(result, Err(Error::TtLib(_))),
			"{range_offset}: {result:?}"
		);
	}
}

fn two_subtables() -> Cmap {
	let mut c4 = make_subtable(4, 3, 1, 0);
	c4.cmap = charmap(&[(0x41, 1), (0x391, 1)]);
	let mut c12 = make_subtable(12, 3, 10, 0);
	c12.cmap = charmap(&[(0x10314, 2)]);
	Cmap {
		table_version: 0,
		tables: vec![c4, c12],
	}
}

#[test]
fn build_reversed() {
	let cmap = two_subtables();
	let expected: BTreeMap<u32, BTreeSet<u32>> = [
		(1, [0x41, 0x391].into_iter().collect()),
		(2, [0x10314].into_iter().collect()),
	]
	.into_iter()
	.collect();
	assert_eq!(cmap.build_reversed(), expected);
	assert_eq!(cmap.build_reversed_min(), map(&[(1, 0x41), (2, 0x10314)]));
}

#[test]
fn get_best_cmap() {
	let cmap = two_subtables();
	assert_eq!(
		cmap.get_best_cmap(&DEFAULT_CMAP_PREFERENCES),
		Some(&map(&[(0x10314, 2)]))
	);
	assert_eq!(
		cmap.get_best_cmap(&[(3, 1)]),
		Some(&map(&[(0x41, 1), (0x391, 1)]))
	);
	assert_eq!(cmap.get_best_cmap(&[(0, 4)]), None);
}

fn strip_variable_items(s: &str) -> String {
	// Python reads the expected files with universal newlines.
	let s = &s.replace("\r\n", "\n");
	match s.find(" ttLibVersion=\"") {
		Some(i) => {
			let rest = &s[i + " ttLibVersion=\"".len()..];
			let end = rest.find('"').map_or(rest.len(), |j| j + 1);
			format!("{}{}", &s[..i], &rest[end..])
		}
		None => s.to_owned(),
	}
}

#[test]
fn format_14() {
	let mut subtable = make_subtable(14, 0, 5, 0);
	let uvs: UvsDict = [
		(0xFE00, vec![(0x30, Some(1))]),
		(0xFE01, vec![(0x30, None)]),
	]
	.into_iter()
	.collect();
	subtable.uvs_dict = Some(uvs.clone());
	let mut font = font_with(names(&[".notdef", "zero.slash"]));
	let mut cmap = Cmap {
		table_version: 0,
		tables: vec![subtable],
	};
	let data = match Table::compile(&mut cmap, &mut font) {
		Ok(d) => d,
		Err(e) => panic!("{e}"),
	};
	let cmap = Cmap::parse(&data).unwrap_or_default();
	assert_eq!(
		cmap.getcmap(0, 5).and_then(|t| t.uvs_dict.clone()),
		Some(uvs.clone())
	);
	font.insert(Box::new(cmap));
	let ttx = font
		.to_ttx(&TtxOptions {
			tables: vec![TableKey::Table(Cmap::TAG)],
			..Default::default()
		})
		.unwrap_or_default();
	let expected =
		std::fs::read_to_string(upstream_data("_c_m_a_p_format_14.ttx")).unwrap_or_default();
	assert_eq!(strip_variable_items(&ttx), strip_variable_items(&expected));

	let bw_compat = std::fs::read_to_string(upstream_data("_c_m_a_p_format_14_bw_compat.ttx"))
		.unwrap_or_default()
		.replace("\r\n", "\n");
	if let Err(e) = font.import_ttx(&bw_compat) {
		panic!("{e}");
	}
	let uvs_read = font
		.get::<Cmap>()
		.ok()
		.flatten()
		.and_then(|c| c.getcmap(0, 5))
		.and_then(|t| t.uvs_dict.clone());
	assert_eq!(uvs_read, Some(uvs));
}

#[test]
fn default_uvs_range_limit() {
	let mut font = font_with(names(&[".notdef", "a"]));
	for count in [255u32, 256, 257, 512, 513, 768] {
		for gaps in [false, true] {
			let mut points: Vec<u32> = (0x1000..0x1000 + count).collect();
			if gaps {
				points.extend(0x2000..0x2000 + count);
			}
			let mut table = make_subtable(14, 0, 5, 0);
			let mut entries: Vec<(u32, Option<u32>)> =
				points.iter().rev().map(|&cp| (cp, None)).collect();
			entries.push((0x3000, Some(1)));
			table.uvs_dict = Some([(0xFE00, entries)].into_iter().collect());
			let data = compile_ok(&table, &mut font);
			// The selector record starts after the 10-byte header.
			let default_offset = be32(&data, 13) as usize;
			let num_ranges = be32(&data, default_offset) as usize;
			let starts: &[u32] = if gaps { &[0x1000, 0x2000] } else { &[0x1000] };
			let expected_ranges: Vec<(u32, u32)> = starts
				.iter()
				.flat_map(|&start| {
					(0..count)
						.step_by(256)
						.map(move |o| (start + o, 256.min(count - o) - 1))
				})
				.collect();
			assert_eq!(num_ranges, expected_ranges.len());
			let actual_ranges: Vec<(u32, u32)> = (0..num_ranges)
				.map(|i| {
					let o = default_offset + 4 + 4 * i;
					(be24(&data, o), data[o + 3] as u32)
				})
				.collect();
			assert_eq!(actual_ranges, expected_ranges);
			let reloaded = reload(14, &data);
			let mut expected: Vec<(u32, Option<u32>)> =
				points.iter().map(|&cp| (cp, None)).collect();
			expected.push((0x3000, Some(1)));
			assert_eq!(
				reloaded.uvs_dict,
				Some([(0xFE00, expected)].into_iter().collect())
			);
			assert_eq!(compile_ok(&reloaded, &mut font), data);
		}
	}
}

#[test]
fn sort_subtables_with_duplicate_keys() {
	let mut s4 = make_subtable(4, 3, 1, 0);
	s4.cmap = charmap(&[(0x41, 1)]);
	let mut s12 = make_subtable(12, 3, 1, 0);
	s12.cmap = charmap(&[(0x41, 1)]);
	let mut cmap = Cmap {
		table_version: 0,
		tables: vec![s12, s4],
	};
	assert!(cmap.sort_tables().is_ok());
	// Stable: the original order is kept.
	assert_eq!(cmap.tables[0].kind, SubtableKind::Format12);
	assert_eq!(cmap.tables.len(), 2);
}

#[test]
fn compile_raises_on_duplicate_subtable_keys() {
	let mut s4 = make_subtable(4, 3, 1, 0);
	s4.cmap = charmap(&[(0x41, 1)]);
	let mut s12 = make_subtable(12, 3, 1, 0);
	s12.cmap = charmap(&[(0x41, 1)]);
	let mut cmap = Cmap {
		table_version: 0,
		tables: vec![s4, s12],
	};
	match Table::compile(&mut cmap, &mut Font::new()) {
		Err(Error::Value(m)) => assert!(m.contains("duplicate"), "{m}"),
		other => panic!("{other:?}"),
	}
}

#[test]
fn unsupported_format_is_preserved() {
	let path = upstream_data("aots/cmap10_font1.otf");
	let data = std::fs::read(path).unwrap_or_default();
	let font = Font::read(data, ReadOptions::default());
	let raw = font
		.ok()
		.and_then(|f| {
			f.reader()
				.and_then(|r| r.table_data(Cmap::TAG).ok())
				.map(|d| d.to_vec())
		})
		.unwrap_or_default();
	let mut cmap = Cmap::parse(&raw).unwrap_or_default();
	let original = cmap.tables[0].data.clone();
	assert_eq!(cmap.tables[0].format, 10);
	let mut font = Font::new();
	let mut writer = XmlWriter::new();
	assert!(TableXml::to_xml(&cmap, &mut writer, &mut font).is_ok());
	let compiled = Table::compile(&mut cmap, &mut font).unwrap_or_default();
	let reloaded = Cmap::parse(&compiled).unwrap_or_default();
	assert_eq!(reloaded.tables[0].format, 10);
	assert_eq!(reloaded.tables[0].kind, SubtableKind::Unknown);
	assert_eq!(reloaded.tables[0].data, original);
}

#[test]
fn shared_offsets_compile_once() {
	// Two directory entries pointing at the same format 4 subtable.
	let mut font = font_with(names(&[".notdef", "a"]));
	let mut t = make_subtable(4, 3, 1, 0);
	t.cmap = charmap(&[(0x41, 1)]);
	let chunk = compile_ok(&t, &mut font);
	let data = directory(2, &[(0, 3, 20), (3, 1, 20)], &chunk);
	let mut cmap = Cmap::parse(&data).unwrap_or_default();
	assert!(Arc::ptr_eq(&cmap.tables[0].cmap, &cmap.tables[1].cmap));
	assert_eq!(Table::compile(&mut cmap, &mut font).ok(), Some(data));
}

#[test]
fn unicode_mappings_of_unicode_subtables() {
	let mut font = font_with(names(&[".notdef", "a", "b"]));
	let mut mac = make_subtable(6, 1, 0, 0);
	mac.cmap = charmap(&[(0x41, 2)]);
	let mut win = make_subtable(4, 3, 1, 0);
	win.cmap = charmap(&[(0x41, 1)]);
	let mut cmap = Cmap {
		table_version: 0,
		tables: vec![mac, win],
	};
	let data = Table::compile(&mut cmap, &mut font).unwrap_or_default();
	let mappings = unicode_mappings(Cmap::TAG, &data).unwrap_or_default();
	assert_eq!(
		mappings,
		vec![UnicodeMapping {
			cmap: map(&[(0x41, 1)])
		}]
	);
}

#[test]
fn split_range_matches_upstream() {
	let cmap: HashMap<i64, i64> = (0x20..0x40)
		.map(|c| (c, if c < 0x30 { c - 0x1F } else { 100 - c }))
		.collect();
	let (start, end) = split_range(0x20, 0x3F, &cmap);
	assert_eq!(start, vec![0x30]);
	assert_eq!(end, vec![0x2F, 0x3F]);
}

#[test]
fn to_unicode_utf16be() {
	let subtable = make_subtable(4, 0, 2, 7);
	assert_eq!(subtable.encoding(), Some("utf_16_be"));
	assert!(subtable.is_unicode());
}

#[test]
fn to_unicode_macroman() {
	let subtable = make_subtable(4, 1, 0, 7);
	assert_eq!(subtable.encoding(), Some("mac_roman"));
	assert!(!subtable.is_unicode());
}

#[test]
fn to_unicode_macromanian() {
	let subtable = make_subtable(4, 1, 0, 37);
	assert!(subtable.encoding().is_some());
	assert!(!subtable.is_unicode());
}

#[test]
fn extended_mac_encodings() {
	// Mac Japanese.
	let subtable = make_subtable(4, 1, 1, 0);
	assert!(subtable.encoding().is_some());
	assert!(!subtable.is_unicode());
}

#[test]
fn extended_unknown() {
	let subtable = make_subtable(4, 10, 11, 12);
	assert_eq!(subtable.encoding(), None);
	// `getEncoding(default)`.
	assert_eq!(subtable.encoding().unwrap_or("ascii"), "ascii");
}

#[test]
fn font_get_best_cmap() {
	let mut font = font_with(names(&[".notdef", "A", "u10314"]));
	font.insert(Box::new(two_subtables()));
	let best = |font: &mut Font, preferences: &[(u16, u16)]| match font.get_best_cmap(preferences) {
		Ok(m) => m,
		Err(e) => panic!("getBestCmap failed: {e}"),
	};
	assert_eq!(
		best(&mut font, &DEFAULT_CMAP_PREFERENCES),
		Some(map(&[(0x10314, 2)]))
	);
	assert_eq!(
		best(&mut font, &[(3, 1)]),
		Some(map(&[(0x41, 1), (0x391, 1)]))
	);
	assert_eq!(best(&mut font, &[(0, 4)]), None);
}

#[test]
fn font_get_best_cmap_applies_dmap_first() {
	let mut font = font_with(names(&[".notdef", "A", "A.alt", "B", "C"]));
	let mut cmap4 = make_subtable(4, 3, 1, 0);
	cmap4.cmap = charmap(&[(0x41, 1), (0x42, 3)]);
	let mut dmap4 = make_subtable(4, 3, 1, 0);
	dmap4.cmap = charmap(&[(0x41, 2), (0x43, 4)]);
	font.insert(Box::new(Cmap {
		table_version: 0,
		tables: vec![cmap4],
	}));
	font.insert(Box::new(Dmap(Cmap {
		table_version: 0,
		tables: vec![dmap4.clone()],
	})));
	assert_eq!(
		font.get_best_cmap(&DEFAULT_CMAP_PREFERENCES).ok().flatten(),
		Some(map(&[(0x41, 2), (0x42, 3), (0x43, 4)]))
	);

	let data = font.table_data(Dmap::TAG).unwrap_or_default();
	let dmap = match Dmap::decompile(&data, &mut font) {
		Ok(t) => t,
		Err(e) => panic!("decompile failed: {e}"),
	};
	assert_eq!(
		dmap.0.get_best_cmap(&DEFAULT_CMAP_PREFERENCES),
		Some(&*dmap4.cmap)
	);
}

#[test]
fn glyph_order_applies_dmap_first() {
	// The upstream test builds a whole font with FontBuilder; only the tables the
	// glyph order comes from matter: maxp, a format 3 post, cmap and DMAP.
	let mut font = font_with(names(&[".notdef", "cmapA", "dmapA", "cmapB", "dmapC"]));
	font.insert(Box::new(crate::tables::Maxp {
		table_version: 0x5000,
		num_glyphs: 5,
		..Default::default()
	}));
	let mut post = crate::tables::Post::default();
	post.header.format_type = 3.0;
	font.insert(Box::new(post));
	let mut cmap4 = make_subtable(4, 3, 1, 0);
	cmap4.cmap = charmap(&[(0x41, 1), (0x42, 3)]);
	font.insert(Box::new(Cmap {
		table_version: 0,
		tables: vec![cmap4],
	}));
	let mut dmap4 = make_subtable(4, 3, 1, 0);
	dmap4.cmap = charmap(&[(0x41, 2), (0x43, 4)]);
	font.insert(Box::new(Dmap(Cmap {
		table_version: 0,
		tables: vec![dmap4],
	})));
	let data = match font.write(&crate::WriteOptions::default()) {
		Ok(d) => d,
		Err(e) => panic!("write failed: {e}"),
	};
	let mut font = match Font::read(data, ReadOptions::default()) {
		Ok(f) => f,
		Err(e) => panic!("read failed: {e}"),
	};
	let order = match font.glyph_order() {
		Ok(o) => o,
		Err(e) => panic!("getGlyphOrder failed: {e}"),
	};
	assert_eq!(
		order.names().to_vec(),
		names(&[".notdef", "glyph00001", "A", "B", "C"])
	);
}
