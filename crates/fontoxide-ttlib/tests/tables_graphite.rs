//! The Graphite tables (`Silf`, `Glat`, `Gloc`, `Feat`, `Sill`) in the versions
//! and with the features that the font of `Tests/ttLib/tables/S__i_l_f_test.py`
//! lacks, compared with fontTools: TTX from the binary, the binary compiled
//! again, and the binary compiled from TTX. The fixtures come from
//! `tools/reference/graphite_fixtures.py`.
//!
//! Compressed tables are compared decompressed: fontTools compresses with LZ4 HC,
//! fontoxide with `lz4_flex`.

#![cfg(feature = "graphite")]
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;

use fontoxide_ttlib::tables::graphite::decompress;
use fontoxide_ttlib::{Font, ReadOptions, TableKey, Tag, TtxOptions, WriteOptions};

const GRAPHITE: [&[u8; 4]; 5] = [b"Feat", b"Glat", b"Gloc", b"Silf", b"Sill"];
const VARIANTS: [&str; 6] = ["base", "v3", "v4", "v5", "v5_uncompressed", "v2_pseudo"];

fn data(name: &str) -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("tests/data/graphite")
		.join(name)
}

fn read(name: &str) -> Font {
	Font::read(std::fs::read(data(name)).unwrap(), ReadOptions::default()).unwrap()
}

/// The TTX without the `ttLibVersion` attribute.
fn normalize(ttx: &str) -> String {
	let ttx = ttx.replace("\r\n", "\n");
	match ttx.find(" ttLibVersion=\"") {
		Some(i) => {
			let rest = &ttx[i + " ttLibVersion=\"".len()..];
			let end = rest.find('"').unwrap() + 1;
			format!("{}{}", &ttx[..i], &rest[end..])
		}
		None => ttx,
	}
}

fn graphite_ttx(font: &mut Font) -> fontoxide_ttlib::Result<String> {
	let options = TtxOptions {
		tables: GRAPHITE
			.iter()
			.map(|t| TableKey::Table(Tag::new(t)))
			.collect(),
		..Default::default()
	};
	font.to_ttx(&options)
}

/// Compares the Graphite tables of two compiled fonts; compressed tables are
/// compared by their header and decompressed content.
fn assert_same_tables(ours: Vec<u8>, expected: &str) {
	let ours = Font::read(ours, ReadOptions::default()).unwrap();
	let theirs = read(expected);
	let (a, b) = (ours.reader().unwrap(), theirs.reader().unwrap());
	for tag in GRAPHITE {
		let tag = Tag::new(tag);
		let (x, y) = (
			a.table_data(tag).unwrap().to_vec(),
			b.table_data(tag).unwrap().to_vec(),
		);
		let compressed = |d: &[u8]| {
			matches!(&tag.0, b"Silf" | b"Glat")
				&& d.len() >= 8
				&& d[..4] >= [0, 3, 0, 0][..]
				&& d[4] >> 3 == 1
		};
		if compressed(&y) {
			assert!(compressed(&x), "{expected} {tag}: not compressed");
			assert_eq!(x[..8], y[..8], "{expected} {tag}: header");
			assert_eq!(
				decompress(&x).unwrap(),
				decompress(&y).unwrap(),
				"{expected} {tag}: decompressed"
			);
		} else {
			assert_eq!(x, y, "{expected} {tag}");
		}
	}
}

#[test]
fn ttx_from_binary() {
	for name in VARIANTS {
		let mut font = read(&format!("{name}.ttf"));
		let result = graphite_ttx(&mut font);
		if data(&format!("{name}.ttx.err")).exists() {
			assert!(result.is_err(), "{name}: decompiles");
			continue;
		}
		let expected = std::fs::read_to_string(data(&format!("{name}.ttx"))).unwrap();
		assert_eq!(normalize(&result.unwrap()), normalize(&expected), "{name}");
	}
}

#[test]
fn binary_roundtrip() {
	for name in VARIANTS {
		if data(&format!("{name}.ttx.err")).exists() {
			continue;
		}
		let mut font = read(&format!("{name}.ttf"));
		for tag in GRAPHITE {
			font.get_dyn(Tag::new(tag)).unwrap();
		}
		let result = font.write(&WriteOptions::default());
		if data(&format!("{name}.roundtrip.err")).exists() {
			assert!(result.is_err(), "{name}: compiles");
			continue;
		}
		assert_same_tables(result.unwrap(), &format!("{name}.roundtrip.ttf"));
	}
}

#[test]
fn binary_from_ttx() {
	for name in VARIANTS {
		if data(&format!("{name}.ttx.err")).exists() {
			continue;
		}
		let mut font = read(&format!("{name}.ttf"));
		for tag in GRAPHITE {
			font.remove(Tag::new(tag));
		}
		let ttx = std::fs::read_to_string(data(&format!("{name}.ttx"))).unwrap();
		font.import_ttx(&ttx).unwrap();
		let result = font.write(&WriteOptions::default());
		if data(&format!("{name}.fromttx.err")).exists() {
			assert!(result.is_err(), "{name}: compiles");
			continue;
		}
		assert_same_tables(result.unwrap(), &format!("{name}.fromttx.ttf"));
	}
}
