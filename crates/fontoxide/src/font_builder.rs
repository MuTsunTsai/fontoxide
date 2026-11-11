//! Building fonts from scratch (`fontTools.fontBuilder`).
//!
//! The setup methods cannot be called in any order, because the tables depend
//! on each other. An order that works:
//!
//! ```text
//! FontBuilder::new(...)
//! setup_glyph_order(...)
//! setup_character_map(...)
//! setup_glyf(...) or setup_cff(...)
//! setup_horizontal_metrics(...)
//! setup_horizontal_header(...)
//! setup_name_table(...)
//! setup_os2(...)
//! setup_post(...)
//! save()
//! ```
//!
//! Like fontTools' keyword arguments, the values of the header tables are given
//! by fontTools field name, as `(name, value)` pairs.
//!
//! Not ported yet: `addOpenTypeFeatures` (feaLib) and the `mappings` of
//! `setupAvar` (avar 2).

use std::collections::{BTreeMap, BTreeSet};

use fontoxide_misc::Tag;
use fontoxide_misc::text_tools::Literal;
use fontoxide_ttlib::cff::charstring::T2CharString;
use fontoxide_ttlib::cff::dict::Value as CffValue;
use fontoxide_ttlib::cff::font::{
	CffFontSet, CharStrings, DictKind, FontDict, PrivateDict, PrivateRef, Strings, TopDict,
	dict_order,
};
use fontoxide_ttlib::tables::cmap::{Cmap, CmapSubtable};
use fontoxide_ttlib::tables::fvar::{Axis, Fvar, NamedInstance};
use fontoxide_ttlib::tables::glyf::{FLAG_CUBIC, Glyf, Glyph};
use fontoxide_ttlib::tables::gvar::Gvar;
use fontoxide_ttlib::tables::hhea::{Hhea, Vhea};
use fontoxide_ttlib::tables::loca::Loca;
use fontoxide_ttlib::tables::maxp::Maxp;
use fontoxide_ttlib::tables::metrics::{Hmtx, Vmtx};
use fontoxide_ttlib::tables::misc_tables::Vorg;
use fontoxide_ttlib::tables::name::{Name, add_multilingual_name};
use fontoxide_ttlib::tables::os2::{Os2, Panose};
use fontoxide_ttlib::tables::post::Post;
use fontoxide_ttlib::tables::tuple_variation::TupleVariation;
use fontoxide_ttlib::tables::{Cff, Cff2, Dsig, Head};
use fontoxide_ttlib::{Error, Font, Result, WriteOptions};
use indexmap::IndexMap;

/// A value for a field of a table, given by fontTools name.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
	/// An integer.
	Int(i64),
	/// A number.
	Float(f64),
	/// A string, such as `achVendID`.
	Str(String),
	/// The `panose` of `OS/2`.
	Panose(Panose),
}

impl From<i64> for Value {
	fn from(v: i64) -> Self {
		Value::Int(v)
	}
}

impl From<i32> for Value {
	fn from(v: i32) -> Self {
		Value::Int(v as i64)
	}
}

impl From<f64> for Value {
	fn from(v: f64) -> Self {
		Value::Float(v)
	}
}

impl From<&str> for Value {
	fn from(v: &str) -> Self {
		Value::Str(v.to_owned())
	}
}

impl Value {
	fn literal(&self) -> Result<Literal> {
		Ok(match self {
			Value::Int(v) => Literal::Int(*v as i128),
			Value::Float(v) => Literal::Float(*v),
			Value::Str(v) => Literal::Str(v.clone()),
			Value::Panose(_) => return Err(Error::value("a panose is only for OS/2's panose")),
		})
	}
}

/// A variation region: (start, peak, end) by axis tag.
pub type Region = BTreeMap<Tag, (f64, f64, f64)>;

/// Field values by fontTools name (the keyword arguments of fontTools).
pub type Values<'a> = &'a [(&'a str, Value)];

fn has(values: Values<'_>, name: &str) -> bool {
	values.iter().any(|(n, _)| *n == name)
}

fn unknown(table: &str, name: &str) -> Error {
	Error::value(format!("'{table}' has no field {name:?}"))
}

/// Sets the fields of an sstruct table.
macro_rules! set_fields {
	($table:expr, $tag:literal, $values:expr) => {{
		for (name, value) in $values {
			if !$table.set_literal(name, &value.literal()?)? {
				return Err(unknown($tag, name));
			}
		}
	}};
}

fn set_os2(os2: &mut Os2, values: Values<'_>) -> Result<()> {
	for (name, value) in values {
		match (*name, value) {
			("panose", Value::Panose(p)) => os2.panose = p.clone(),
			("achVendID", Value::Str(s)) => {
				if !s.is_ascii() {
					return Err(Error::value("'ascii' codec can't encode achVendID"));
				}
				let mut id = [b' '; 4];
				for (d, b) in id.iter_mut().zip(s.bytes()) {
					*d = b;
				}
				os2.base.ach_vend_id = id;
			}
			_ => {
				let lit = value.literal()?;
				let int = || {
					lit.as_int()
						.ok_or_else(|| Error::value(format!("invalid {name}")))
				};
				let float = || {
					lit.as_float()
						.ok_or_else(|| Error::value(format!("invalid {name}")))
				};
				match *name {
					"ulCodePageRange1" => {
						os2.ul_code_page_range1 = fontoxide_ttlib::binary::fit(int()? as i64, name)?
					}
					"ulCodePageRange2" => {
						os2.ul_code_page_range2 = fontoxide_ttlib::binary::fit(int()? as i64, name)?
					}
					"sxHeight" => {
						os2.sx_height = fontoxide_ttlib::binary::fit(int()? as i64, name)?
					}
					"sCapHeight" => {
						os2.s_cap_height = fontoxide_ttlib::binary::fit(int()? as i64, name)?
					}
					"usDefaultChar" => {
						os2.us_default_char = fontoxide_ttlib::binary::fit(int()? as i64, name)?
					}
					"usBreakChar" => {
						os2.us_break_char = fontoxide_ttlib::binary::fit(int()? as i64, name)?
					}
					"usMaxContext" => {
						os2.us_max_context = fontoxide_ttlib::binary::fit(int()? as i64, name)?
					}
					"usLowerOpticalPointSize" => os2.us_lower_optical_point_size = float()?,
					"usUpperOpticalPointSize" => os2.us_upper_optical_point_size = float()?,
					_ => {
						if !os2.base.set_literal(name, &lit)? {
							return Err(unknown("OS/2", name));
						}
					}
				}
			}
		}
	}
	Ok(())
}

/// The default `head` values (`_headDefaults`).
const HEAD_DEFAULTS: &[(&str, f64)] = &[
	("tableVersion", 1.0),
	("fontRevision", 1.0),
	("checkSumAdjustment", 0.0),
	("magicNumber", 1_594_834_165.0),
	("flags", 3.0),
	("unitsPerEm", 1000.0),
	("created", 0.0),
	("modified", 0.0),
	("xMin", 0.0),
	("yMin", 0.0),
	("xMax", 0.0),
	("yMax", 0.0),
	("macStyle", 0.0),
	("lowestRecPPEM", 3.0),
	("fontDirectionHint", 2.0),
	("indexToLocFormat", 0.0),
	("glyphDataFormat", 0.0),
];

/// The default `hhea` values (`_hheaDefaults`).
const HHEA_DEFAULTS: &[(&str, i64)] = &[
	("tableVersion", 0x0001_0000),
	("ascent", 0),
	("descent", 0),
	("lineGap", 0),
	("advanceWidthMax", 0),
	("minLeftSideBearing", 0),
	("minRightSideBearing", 0),
	("xMaxExtent", 0),
	("caretSlopeRise", 1),
	("caretSlopeRun", 0),
	("caretOffset", 0),
	("reserved0", 0),
	("reserved1", 0),
	("reserved2", 0),
	("reserved3", 0),
	("metricDataFormat", 0),
	("numberOfHMetrics", 0),
];

/// The default `vhea` values (`_vheaDefaults`).
const VHEA_DEFAULTS: &[(&str, i64)] = &[
	("tableVersion", 0x0001_0000),
	("ascent", 0),
	("descent", 0),
	("lineGap", 0),
	("advanceHeightMax", 0),
	("minTopSideBearing", 0),
	("minBottomSideBearing", 0),
	("yMaxExtent", 0),
	("caretSlopeRise", 0),
	("caretSlopeRun", 0),
	("reserved0", 0),
	("reserved1", 0),
	("reserved2", 0),
	("reserved3", 0),
	("reserved4", 0),
	("metricDataFormat", 0),
	("numberOfVMetrics", 0),
];

/// The descriptive names of name IDs (`_nameIDs`).
pub const NAME_IDS: &[(&str, u16)] = &[
	("copyright", 0),
	("familyName", 1),
	("styleName", 2),
	("uniqueFontIdentifier", 3),
	("fullName", 4),
	("version", 5),
	("psName", 6),
	("trademark", 7),
	("manufacturer", 8),
	("designer", 9),
	("description", 10),
	("vendorURL", 11),
	("designerURL", 12),
	("licenseDescription", 13),
	("licenseInfoURL", 14),
	("typographicFamily", 16),
	("typographicSubfamily", 17),
	("compatibleFullName", 18),
	("sampleText", 19),
	("postScriptCIDFindfontName", 20),
	("wwsFamilyName", 21),
	("wwsSubfamilyName", 22),
	("lightBackgroundPalette", 23),
	("darkBackgroundPalette", 24),
	("variationsPostScriptNamePrefix", 25),
];

/// A name ID, or its descriptive name (see [`NAME_IDS`]).
#[derive(Debug, Clone, PartialEq)]
pub enum NameKey {
	/// A name ID.
	Id(u16),
	/// A descriptive name, such as `familyName`.
	Name(String),
}

impl From<u16> for NameKey {
	fn from(id: u16) -> Self {
		NameKey::Id(id)
	}
}

impl From<&str> for NameKey {
	fn from(name: &str) -> Self {
		NameKey::Name(name.to_owned())
	}
}

/// A name: in English, or in several languages (BCP 47 tags and strings).
#[derive(Debug, Clone, PartialEq)]
pub enum NameValue {
	/// An English name.
	English(String),
	/// A name in several languages.
	Multilingual(Vec<(String, String)>),
}

impl From<&str> for NameValue {
	fn from(s: &str) -> Self {
		NameValue::English(s.to_owned())
	}
}

impl NameValue {
	fn languages(&self) -> Vec<(&str, &str)> {
		match self {
			NameValue::English(s) => vec![("en", s.as_str())],
			NameValue::Multilingual(v) => v.iter().map(|(l, s)| (l.as_str(), s.as_str())).collect(),
		}
	}
}

/// An axis of `fvar`: tag, minimum, default, maximum and name (the tuples of
/// `setupFvar`).
#[derive(Debug, Clone, PartialEq)]
pub struct FvarAxis {
	/// The axis tag.
	pub tag: Tag,
	/// The minimum value.
	pub minimum: f64,
	/// The default value.
	pub default: f64,
	/// The maximum value.
	pub maximum: f64,
	/// The axis name.
	pub name: NameValue,
	/// Whether the axis is hidden (`hidden` of a designspace axis).
	pub hidden: bool,
}

/// A named instance of `fvar` (the dicts of `setupFvar`).
#[derive(Debug, Clone, PartialEq)]
pub struct FvarInstance {
	/// The coordinates by axis tag (`location`).
	pub location: Vec<(Tag, f64)>,
	/// The style name (`stylename`).
	pub style_name: NameValue,
	/// The PostScript name (`postscriptfontname`).
	pub postscript_font_name: Option<String>,
}

/// Builds fonts from scratch (`FontBuilder`).
#[derive(Debug)]
pub struct FontBuilder {
	/// The font being built (`font`).
	pub font: Font,
	/// Whether the font has TrueType outlines (`isTTF`).
	pub is_ttf: bool,
	/// Whether the beyond-64k companion tables are built where they exist
	/// (`beyond64k`).
	pub beyond64k: bool,
}

impl FontBuilder {
	/// A builder for a new font (`FontBuilder(unitsPerEm, isTTF=..., beyond64k=...)`):
	/// TrueType or CFF outlines, with `head` and `maxp` set up. The timestamps
	/// are not recalculated when saving.
	pub fn new(units_per_em: u16, is_ttf: bool, beyond64k: bool) -> Result<Self> {
		let mut font = Font::new();
		font.recalc_timestamp = false;
		let mut builder = FontBuilder {
			font,
			is_ttf,
			beyond64k,
		};
		let now = fontoxide_misc::time_tools::timestamp_now()
			.ok_or_else(|| Error::value("cannot get the current time"))?;
		builder.setup_head(&[
			("unitsPerEm", Value::Int(units_per_em as i64)),
			("created", Value::Int(now)),
			("modified", Value::Int(now)),
		])?;
		builder.setup_maxp()?;
		Ok(builder)
	}

	/// A builder for an existing font (`FontBuilder(font=font)`). It has TrueType
	/// outlines if it has `glyf` or `GLYF`, and beyond-64k tables if asked or if
	/// it has any.
	pub fn from_font(font: Font, beyond64k: bool) -> Self {
		let is_ttf = font.contains(Tag::new(b"glyf")) || font.contains(Tag::new(b"GLYF"));
		let beyond64k = beyond64k
			|| [
				b"GLYF", b"GVAR", b"HHEA", b"HMTX", b"LOCA", b"MAXP", b"VHEA", b"VMTX",
			]
			.iter()
			.any(|t| font.contains(Tag::new(t)));
		FontBuilder {
			font,
			is_ttf,
			beyond64k,
		}
	}

	/// The compiled font (`save`).
	pub fn save(&mut self) -> Result<Vec<u8>> {
		self.font.write(&WriteOptions::default())
	}

	/// Saves the font to a file (`save(path)`).
	pub fn save_to(&mut self, path: impl AsRef<std::path::Path>) -> Result<()> {
		let data = self.save()?;
		std::fs::write(path, data)?;
		Ok(())
	}

	/// Creates `head` with the default values and `values` (`setupHead`).
	pub fn setup_head(&mut self, values: Values<'_>) -> Result<()> {
		let mut head = Head::default();
		for &(name, v) in HEAD_DEFAULTS {
			head.set_literal(name, &literal_number(v))?;
		}
		set_fields!(head, "head", values);
		self.font.insert(Box::new(head));
		Ok(())
	}

	/// Sets fields of `head` (`updateHead`).
	pub fn update_head(&mut self, values: Values<'_>) -> Result<()> {
		let head = self.font.require::<Head>()?;
		set_fields!(head, "head", values);
		Ok(())
	}

	/// Sets the glyph order (`setupGlyphOrder`).
	pub fn setup_glyph_order(&mut self, glyph_order: Vec<String>) {
		self.font.set_glyph_order(glyph_order);
	}

	/// Builds the `cmap` table from code points to glyph names
	/// (`setupCharacterMap`), with Unicode variation sequences (code point,
	/// variation selector, glyph name or `None` for the default glyph) if
	/// given.
	///
	/// A format 4 subtable that overflows is an error, unless `allow_fallback`,
	/// which uses format 12 instead.
	pub fn setup_character_map(
		&mut self,
		cmapping: &BTreeMap<u32, String>,
		uvs: Option<&[(u32, u32, Option<String>)]>,
		allow_fallback: bool,
	) -> Result<()> {
		let mut sub_tables = Vec::new();
		let highest_unicode = cmapping.keys().next_back().copied().unwrap_or(0);
		let mut gids = BTreeMap::new();
		for (&u, name) in cmapping {
			gids.insert(u, self.font.glyph_id(name)?);
		}
		let high_glyph_ids = gids.values().any(|&g| g > 0xFFFF);
		let mut cmapping_3_1 = gids.clone();
		if high_glyph_ids {
			sub_tables.push(build_cmap_subtable(&gids, 12, 3, 10));
			sub_tables.push(build_cmap_subtable(&gids, 12, 0, 4));
		} else if highest_unicode > 0xFFFF {
			cmapping_3_1.retain(|&k, _| k < 0x10000);
			sub_tables.push(build_cmap_subtable(&gids, 12, 3, 10));
		}
		if !high_glyph_ids {
			let mut format = 4;
			let mut sub_table_3_1 = build_cmap_subtable(&cmapping_3_1, format, 3, 1);
			match sub_table_3_1.compile(&mut self.font) {
				Ok(_) => {}
				Err(Error::Struct(_)) => {
					// Format 4 overflowed; fall back to format 12.
					if !allow_fallback {
						return Err(Error::value(
							"cmap format 4 subtable overflowed; sort glyph order by unicode to fix.",
						));
					}
					format = 12;
					sub_table_3_1 = build_cmap_subtable(&cmapping_3_1, format, 3, 1);
				}
				Err(e) => return Err(e),
			}
			sub_tables.push(sub_table_3_1);
			sub_tables.push(build_cmap_subtable(&cmapping_3_1, format, 0, 3));
		}
		if let Some(uvs) = uvs {
			let mut uvs_dict: IndexMap<u32, Vec<(u32, Option<u32>)>> = IndexMap::new();
			for (unicode, selector, name) in uvs {
				// A sequence for the glyph of the cmap is a default variation.
				let name = name.as_ref().filter(|n| cmapping.get(unicode) != Some(*n));
				let gid = name.map(|n| self.font.glyph_id(n)).transpose()?;
				uvs_dict.entry(*selector).or_default().push((*unicode, gid));
			}
			let format = if uvs_dict
				.values()
				.flatten()
				.any(|(_, g)| g.is_some_and(|g| g > 0xFFFF))
			{
				15
			} else {
				14
			};
			let mut uvs_sub_table = build_cmap_subtable(&BTreeMap::new(), format, 0, 5);
			uvs_sub_table.uvs_dict = Some(uvs_dict.into_iter().collect());
			sub_tables.push(uvs_sub_table);
		}
		self.font.insert(Box::new(Cmap {
			table_version: 0,
			tables: sub_tables,
		}));
		Ok(())
	}

	/// Creates the `name` table (`setupNameTable`) from name IDs or descriptive
	/// names (see [`NAME_IDS`]) to names, for Windows and Macintosh as asked.
	pub fn setup_name_table(
		&mut self,
		name_strings: &[(NameKey, NameValue)],
		windows: bool,
		mac: bool,
	) -> Result<()> {
		self.font.insert(Box::new(Name::default()));
		for (key, value) in name_strings {
			let name_id = match key {
				NameKey::Id(id) => *id,
				NameKey::Name(n) => NAME_IDS
					.iter()
					.find(|(k, _)| k == n)
					.map(|(_, id)| *id)
					.ok_or_else(|| Error::value(format!("KeyError: '{n}'")))?,
			};
			add_multilingual_name(
				&mut self.font,
				&value.languages(),
				Some(name_id),
				windows,
				mac,
				0,
			)?;
		}
		Ok(())
	}

	/// Creates `OS/2` with the default values and `values` (`setupOS2`).
	/// `xAvgCharWidth` is calculated from the horizontal metrics and the
	/// Unicode ranges from `cmap`, unless given.
	pub fn setup_os2(&mut self, values: Values<'_>) -> Result<()> {
		let mut os2 = Os2::default();
		set_os2(&mut os2, &os2_defaults())?;
		set_os2(&mut os2, values)?;
		if !has(values, "xAvgCharWidth") {
			let hmtx = self.font.get::<Hmtx>()?.ok_or_else(|| {
				Error::Assertion(
					"the 'hmtx' or 'HMTX' table must be setup before the 'OS/2' table".into(),
				)
			})?;
			let widths: Vec<u16> = hmtx.metrics.values().map(|m| m.0).collect();
			os2.recalc_avg_char_width(widths);
		}
		// fontTools checks ulUnicodeRange3 twice and never ulUnicodeRange4.
		if !(has(values, "ulUnicodeRange1")
			|| has(values, "ulUnicodeRange2")
			|| has(values, "ulUnicodeRange3"))
		{
			let cmap = self.font.get::<Cmap>()?.ok_or_else(|| {
				Error::Assertion("the 'cmap' table must be setup before the 'OS/2' table".into())
			})?;
			let unicodes: BTreeSet<u32> = cmap
				.tables
				.iter()
				.filter(|t| t.is_unicode())
				.flat_map(|t| t.cmap.keys().copied())
				.collect();
			os2.recalc_unicode_ranges(&unicodes, false)?;
		}
		self.font.insert(Box::new(os2));
		Ok(())
	}

	/// Creates a `CFF ` table (`setupCFF`) with the PostScript name, Top DICT
	/// entries (`FontMatrix` defaults to 1/unitsPerEm), charstrings by glyph
	/// name and Private DICT entries.
	pub fn setup_cff(
		&mut self,
		ps_name: &str,
		font_info: &[(&str, CffValue)],
		char_strings: IndexMap<String, T2CharString>,
		private_dict: &[(&str, CffValue)],
	) -> Result<()> {
		if self.is_ttf {
			return Err(Error::Assertion("not self.isTTF".into()));
		}
		self.font.sfnt_version = Tag::new(b"OTTO");
		let mut private = PrivateDict::default();
		for (key, value) in private_dict {
			private
				.values
				.insert(dict_name(DictKind::Private, false, key)?, value.clone());
		}
		let mut top = TopDict {
			charset: Some(self.font.glyph_order()?.names().to_vec()),
			private: Some(Box::new(private)),
			..Default::default()
		};
		for (key, value) in font_info {
			top.values
				.insert(dict_name(DictKind::Top, false, key)?, value.clone());
		}
		if !font_info.iter().any(|(k, _)| *k == "FontMatrix") {
			let scale = 1.0 / self.font.require::<Head>()?.units_per_em as f64;
			top.values.insert("FontMatrix", font_matrix(scale));
		}
		top.char_strings = Some(char_strings_of(char_strings, PrivateRef::Top, false));
		let mut set = CffFontSet::default();
		set.major = 1;
		set.minor = 0;
		set.font_names = vec![ps_name.to_owned()];
		set.top_dicts = vec![top];
		set.strings = Some(Strings::default());
		self.font.insert(Box::new(Cff { cff: set }));
		Ok(())
	}

	/// Creates a `CFF2` table (`setupCFF2`) with charstrings by glyph name, a
	/// font DICT for each Private DICT of `fd_array_list` (one empty one by
	/// default), and the variation regions if given (see
	/// [`FontBuilder::setup_cff2_regions`]).
	pub fn setup_cff2(
		&mut self,
		char_strings: IndexMap<String, T2CharString>,
		fd_array_list: Option<&[Vec<(&str, CffValue)>]>,
		regions: Option<&[Region]>,
	) -> Result<()> {
		if self.is_ttf {
			return Err(Error::Assertion("not self.isTTF".into()));
		}
		self.font.sfnt_version = Tag::new(b"OTTO");
		let default = [Vec::new()];
		let fd_array_list = fd_array_list.unwrap_or(&default);
		let mut fd_array = Vec::new();
		for private_dict in fd_array_list {
			let mut private = PrivateDict {
				cff2: true,
				..Default::default()
			};
			for (key, value) in private_dict {
				private
					.values
					.insert(dict_name(DictKind::Private, true, key)?, value.clone());
			}
			fd_array.push(FontDict {
				cff2: true,
				private: Some(Box::new(private)),
				..Default::default()
			});
		}
		let scale = 1.0 / self.font.require::<Head>()?.units_per_em as f64;
		let mut top = TopDict {
			cff2: true,
			fd_array: Some(fd_array),
			uses_font_glyph_order: true,
			..Default::default()
		};
		top.values.insert("FontMatrix", font_matrix(scale));
		top.char_strings = Some(char_strings_of(char_strings, PrivateRef::Fd(0), true));
		let mut set = CffFontSet::default();
		set.major = 2;
		set.minor = 0;
		set.font_names = vec!["CFF2Font".to_owned()];
		set.top_dicts = vec![top];
		set.strings = None;
		self.font.insert(Box::new(Cff2 { cff: set }));
		if let Some(regions) = regions.filter(|r| !r.is_empty()) {
			self.setup_cff2_regions(regions)?;
		}
		Ok(())
	}

	/// Sets the variation regions of `CFF2`, by axis tag: (start, peak, end)
	/// (`setupCFF2Regions`). `fvar` and `CFF2` must be set up first.
	pub fn setup_cff2_regions(&mut self, regions: &[Region]) -> Result<()> {
		use fontoxide_ttlib::ot::var_store::{
			build_var_data, build_var_region_list, build_var_store,
		};
		let axis_tags: Vec<Tag> = self
			.font
			.get::<Fvar>()?
			.ok_or_else(|| Error::Assertion("fvar must to be set up first".into()))?
			.axes
			.iter()
			.map(|a| a.tag())
			.collect();
		if !self.font.contains(Tag::new(b"CFF2")) {
			return Err(Error::Assertion("CFF2 must to be set up first".into()));
		}
		let supports: Vec<BTreeMap<String, (f64, f64, f64)>> = regions
			.iter()
			.map(|r| r.iter().map(|(t, v)| (t.as_string(), *v)).collect())
			.collect();
		let tags: Vec<String> = axis_tags.iter().map(Tag::as_string).collect();
		let region_list = build_var_region_list(&supports, &tags)?;
		let indices: Vec<u16> = (0..regions.len() as u16).collect();
		let var_data = build_var_data(indices, Vec::new(), false);
		let store = build_var_store(region_list, vec![var_data]);
		let cff2 = self.font.require::<Cff2>()?;
		let top = cff2
			.cff
			.top_dicts
			.first_mut()
			.ok_or_else(|| Error::ttlib("CFF2 has no Top DICT"))?;
		top.var_store = Some(Box::new(store));
		Ok(())
	}

	/// Creates the `glyf` and `loca` tables from glyphs by name (`setupGlyf`),
	/// and calculates their bounds unless `calc_glyph_bounds` is false.
	///
	/// Glyphs with cubic curves are an error, except in beyond-64k fonts, whose
	/// `GLYF` table supports them.
	pub fn setup_glyf(
		&mut self,
		glyphs: IndexMap<String, Glyph>,
		calc_glyph_bounds: bool,
	) -> Result<()> {
		if !self.is_ttf {
			return Err(Error::Assertion("self.isTTF".into()));
		}
		for (name, g) in &glyphs {
			if !self.beyond64k
				&& g.number_of_contours > 0
				&& g.flags.iter().any(|f| f & FLAG_CUBIC != 0)
			{
				return Err(Error::value(format!(
					"Glyph {} has cubic Bezier outlines; convert to quadratics with cu2qu.",
					fontoxide_misc::py::str_repr(name)
				)));
			}
		}
		let order = self.font.glyph_order()?;
		let mut glyf = Glyf {
			extended: self.beyond64k,
			..Default::default()
		};
		let mut gids = Vec::with_capacity(glyphs.len());
		for (name, g) in glyphs {
			let gid = self.font.glyph_id(&name)?;
			glyf.glyphs.insert(gid, g);
			gids.push(gid);
		}
		if calc_glyph_bounds {
			for gid in gids {
				glyf.recalc_glyph_bounds(gid, &order)?;
			}
		}
		let loca = Loca {
			extended: self.beyond64k,
			..Default::default()
		};
		self.font.insert(Box::new(loca));
		self.font.insert(Box::new(glyf));
		Ok(())
	}

	/// Adds an `fvar` table (`setupFvar`); see [`add_fvar`].
	pub fn setup_fvar(&mut self, axes: &[FvarAxis], instances: &[FvarInstance]) -> Result<()> {
		add_fvar(&mut self.font, axes, instances)
	}

	/// Adds an `avar` table for the axis mappings of designspace axes
	/// (`setupAvar` without `mappings`). `fvar` must be set up first.
	pub fn setup_avar(&mut self, axes: &[fontoxide_varlib::avar_builder::AvarAxis]) -> Result<()> {
		if !self.font.contains(Tag::new(b"fvar")) {
			return Err(Error::value(
				"KeyError: \"'fvar' table is missing; can't add 'avar'.\"",
			));
		}
		fontoxide_varlib::avar_builder::add_avar(&mut self.font, axes)?;
		Ok(())
	}

	/// Creates the `gvar` table (or `GVAR`) from the variations of each glyph by
	/// name (`setupGvar`).
	pub fn setup_gvar(&mut self, variations: IndexMap<String, Vec<TupleVariation>>) -> Result<()> {
		let mut list = Vec::with_capacity(variations.len());
		for (name, v) in variations {
			list.push((self.font.glyph_id(&name)?, v));
		}
		list.sort_by_key(|(g, _)| *g);
		let gvar = Gvar {
			version: 1,
			reserved: 0,
			variations: list,
			extended: self.beyond64k,
			..Default::default()
		};
		self.font.insert(Box::new(gvar));
		Ok(())
	}

	/// Calculates the bounds of all glyphs of `glyf` (`calcGlyphBounds`).
	pub fn calc_glyph_bounds(&mut self) -> Result<()> {
		let order = self.font.glyph_order()?;
		let glyf = self.font.require::<Glyf>()?;
		let gids: Vec<u32> = glyf.glyphs.keys().copied().collect();
		for gid in gids {
			glyf.recalc_glyph_bounds(gid, &order)?;
		}
		Ok(())
	}

	/// Creates `hmtx` (or `HMTX`) from glyph names to (advance width, left side
	/// bearing) (`setupHorizontalMetrics`).
	pub fn setup_horizontal_metrics(
		&mut self,
		metrics: &IndexMap<String, (f64, f64)>,
	) -> Result<()> {
		let mut table = Hmtx {
			extended: self.beyond64k,
			..Default::default()
		};
		table.metrics = self.rounded_metrics(metrics)?;
		self.font.insert(Box::new(table));
		Ok(())
	}

	/// Creates `vmtx` (or `VMTX`) from glyph names to (advance height, top side
	/// bearing) (`setupVerticalMetrics`).
	pub fn setup_vertical_metrics(&mut self, metrics: &IndexMap<String, (f64, f64)>) -> Result<()> {
		let mut table = Vmtx {
			extended: self.beyond64k,
			..Default::default()
		};
		table.metrics = self.rounded_metrics(metrics)?;
		self.font.insert(Box::new(table));
		Ok(())
	}

	/// `int(round(w)), int(round(lsb))` of each glyph.
	fn rounded_metrics(
		&mut self,
		metrics: &IndexMap<String, (f64, f64)>,
	) -> Result<BTreeMap<u32, (u16, i16)>> {
		let mut out = BTreeMap::new();
		for (name, &(w, lsb)) in metrics {
			let round = |v: f64| fontoxide_misc::py::round(v) as i64;
			let advance = fontoxide_ttlib::binary::fit(round(w), "advance")?;
			let side_bearing = fontoxide_ttlib::binary::fit(round(lsb), "sideBearing")?;
			out.insert(self.font.glyph_id(name)?, (advance, side_bearing));
		}
		Ok(out)
	}

	/// Creates `hhea` (or `HHEA`) with the default values and `values`
	/// (`setupHorizontalHeader`).
	pub fn setup_horizontal_header(&mut self, values: Values<'_>) -> Result<()> {
		let mut hhea = Hhea {
			extended: self.beyond64k,
			..Default::default()
		};
		for &(name, v) in HHEA_DEFAULTS {
			hhea.set_literal(name, &Literal::Int(v as i128))?;
		}
		set_fields!(hhea, "hhea", values);
		self.font.insert(Box::new(hhea));
		Ok(())
	}

	/// Creates `vhea` (or `VHEA`) with the default values and `values`
	/// (`setupVerticalHeader`).
	pub fn setup_vertical_header(&mut self, values: Values<'_>) -> Result<()> {
		let mut vhea = Vhea {
			extended: self.beyond64k,
			..Default::default()
		};
		for &(name, v) in VHEA_DEFAULTS {
			vhea.set_literal(name, &Literal::Int(v as i128))?;
		}
		set_fields!(vhea, "vhea", values);
		self.font.insert(Box::new(vhea));
		Ok(())
	}

	/// Creates `VORG` from glyph names to vertical origins
	/// (`setupVerticalOrigins`). The default origin is the most frequent one
	/// unless given.
	pub fn setup_vertical_origins(
		&mut self,
		vertical_origins: &IndexMap<String, i16>,
		default: Option<i16>,
	) -> Result<()> {
		let default = match default {
			Some(d) => d,
			None => {
				let mut bag: IndexMap<i16, usize> = IndexMap::new();
				for &v in vertical_origins.values() {
					*bag.entry(v).or_default() += 1;
				}
				let mut items: Vec<(i16, usize)> = bag.into_iter().collect();
				// A stable sort by decreasing count keeps the first one of a tie.
				items.sort_by_key(|i| std::cmp::Reverse(i.1));
				items
					.first()
					.map(|i| i.0)
					.ok_or_else(|| Error::value("IndexError: list index out of range"))?
			}
		};
		let mut vorg = Vorg {
			major_version: if self.beyond64k { 2 } else { 1 },
			minor_version: 0,
			default_vert_origin_y: default,
			..Default::default()
		};
		for (name, &v) in vertical_origins {
			vorg.set(self.font.glyph_id(name)?, v);
		}
		self.font.insert(Box::new(vorg));
		Ok(())
	}

	/// Creates `post` with the default values and `values` (`setupPost`):
	/// format 2 with glyph names for TrueType and CFF2 fonts if
	/// `keep_glyph_names`, format 3 otherwise. Beyond-64k fonts with more than
	/// 65535 glyphs get format 3.
	pub fn setup_post(&mut self, keep_glyph_names: bool, values: Values<'_>) -> Result<()> {
		let is_cff2 = self.font.contains(Tag::new(b"CFF2"));
		let mut post = Post::default();
		for (name, v) in [
			("formatType", 3.0),
			("italicAngle", 0.0),
			("underlinePosition", 0.0),
			("underlineThickness", 0.0),
			("isFixedPitch", 0.0),
			("minMemType42", 0.0),
			("maxMemType42", 0.0),
			("minMemType1", 0.0),
			("maxMemType1", 0.0),
		] {
			post.header.set_literal(name, &literal_number(v))?;
		}
		set_fields!(post.header, "post", values);
		if (self.is_ttf || is_cff2)
			&& keep_glyph_names
			&& (!self.beyond64k || self.font.glyph_order()?.len() <= 0xFFFF)
		{
			post.header.format_type = 2.0;
			post.extra_names = Some(Vec::new());
			post.mapping = Some(BTreeMap::new());
		} else {
			post.header.format_type = 3.0;
		}
		self.font.insert(Box::new(post));
		Ok(())
	}

	/// Creates `maxp` (or `MAXP`) with the default values (`setupMaxp`).
	pub fn setup_maxp(&mut self) -> Result<()> {
		let maxp = Maxp {
			table_version: if self.is_ttf {
				0x0001_0000
			} else {
				0x0000_5000
			},
			extended: self.beyond64k,
			v1: fontoxide_ttlib::tables::maxp::MaxpV1 {
				max_zones: if self.is_ttf { 2 } else { 0 },
				..Default::default()
			},
			..Default::default()
		};
		self.font.insert(Box::new(maxp));
		Ok(())
	}

	/// Adds an empty `DSIG` table, which some Microsoft applications want; it
	/// does not sign the font (`setupDummyDSIG`).
	pub fn setup_dummy_dsig(&mut self) {
		self.font.insert(Box::new(Dsig {
			version: 1,
			num_sigs: 0,
			flag: 0,
			signature_records: Some(Vec::new()),
		}));
	}

	/// Adds OpenType features to the font from a string in feature file syntax
	/// (`addOpenTypeFeatures`); see
	/// [`fontoxide_fealib::builder::add_open_type_features_from_string`].
	///
	/// `filename` is used in error messages and to find included files.
	/// `tables` restricts the layout tables built. `debug` adds source
	/// debugging information to the font in the `Debg` table.
	pub fn add_open_type_features(
		&mut self,
		features: &str,
		filename: Option<&str>,
		tables: Option<&[&str]>,
		debug: bool,
	) -> fontoxide_fealib::Result<()> {
		fontoxide_fealib::builder::add_open_type_features_from_string(
			&mut self.font,
			features,
			filename,
			tables,
			debug,
		)
	}

	/// Adds conditional substitutions to a variable font (`addFeatureVariations`);
	/// see [`fontoxide_varlib::feature_vars::add_feature_variations`]. `fvar`
	/// must be set up first.
	pub fn add_feature_variations(
		&mut self,
		conditional_substitutions: &[(
			fontoxide_varlib::feature_vars::Region,
			fontoxide_varlib::feature_vars::Substitution,
		)],
		feature_tag: Tag,
	) -> Result<()> {
		if !self.font.contains(Tag::new(b"fvar")) {
			return Err(Error::value(
				"KeyError: \"'fvar' table is missing; can't add FeatureVariations.\"",
			));
		}
		fontoxide_varlib::feature_vars::add_feature_variations(
			&mut self.font,
			conditional_substitutions,
			&[feature_tag],
		)
	}

	/// Builds a new `COLR` table from color layers by base glyph name
	/// (`setupCOLR`); see [`fontoxide_colorlib::build_colr`].
	pub fn setup_colr(
		&mut self,
		color_layers: indexmap::IndexMap<String, fontoxide_colorlib::Value>,
		options: fontoxide_colorlib::BuildColrOptions,
	) -> fontoxide_colorlib::Result<()> {
		let glyphs = self.font.glyph_order()?.reverse_map().clone();
		let options = fontoxide_colorlib::BuildColrOptions {
			glyph_map: true,
			..options
		};
		let colr = fontoxide_colorlib::build_colr(color_layers, &glyphs, options)?;
		self.font.insert(Box::new(colr));
		Ok(())
	}

	/// Builds a new `CPAL` table from palettes of (red, green, blue, alpha)
	/// colors in the range 0 to 1 (`setupCPAL`); see
	/// [`fontoxide_colorlib::build_cpal`]. Labels are added to the `name`
	/// table.
	pub fn setup_cpal(
		&mut self,
		palettes: &[Vec<[f64; 4]>],
		palette_types: Option<&[fontoxide_colorlib::ColorPaletteType]>,
		palette_labels: Option<&[Option<fontoxide_colorlib::LocalizedString>]>,
		palette_entry_labels: Option<&[Option<fontoxide_colorlib::LocalizedString>]>,
	) -> fontoxide_colorlib::Result<()> {
		let name = self.font.get::<Name>()?;
		let cpal = fontoxide_colorlib::build_cpal(
			palettes,
			palette_types,
			palette_labels,
			palette_entry_labels,
			name,
		)?;
		self.font.insert(Box::new(cpal));
		Ok(())
	}

	/// Builds a `STAT` table (`setupStat`); see
	/// [`fontoxide_otllib::builder::build_stat_table`]. Macintosh names are added
	/// if the `name` table has any.
	pub fn setup_stat(
		&mut self,
		axes: &[fontoxide_otllib::builder::StatAxis],
		locations: Option<&[fontoxide_otllib::builder::StatLocation]>,
		elided_fallback_name: &fontoxide_otllib::builder::StatName,
	) -> fontoxide_otllib::Result<()> {
		let name = self
			.font
			.get::<Name>()?
			.ok_or_else(|| Error::Assertion("name must to be set up first".into()))?;
		let mac_names = name.names.iter().any(|n| n.platform_id == 1);
		fontoxide_otllib::builder::build_stat_table(
			&mut self.font,
			axes,
			locations,
			elided_fallback_name,
			true,
			mac_names,
		)
	}
}

fn literal_number(v: f64) -> Literal {
	if v.fract() == 0.0 {
		Literal::Int(v as i128)
	} else {
		Literal::Float(v)
	}
}

/// The default `OS/2` values (`_getOS2Defaults`).
fn os2_defaults() -> Vec<(&'static str, Value)> {
	let ints: &[(&str, i64)] = &[
		("version", 3),
		("xAvgCharWidth", 0),
		("usWeightClass", 400),
		("usWidthClass", 5),
		("fsType", 0x0004),
		("ySubscriptXSize", 0),
		("ySubscriptYSize", 0),
		("ySubscriptXOffset", 0),
		("ySubscriptYOffset", 0),
		("ySuperscriptXSize", 0),
		("ySuperscriptYSize", 0),
		("ySuperscriptXOffset", 0),
		("ySuperscriptYOffset", 0),
		("yStrikeoutSize", 0),
		("yStrikeoutPosition", 0),
		("sFamilyClass", 0),
		("ulUnicodeRange1", 0),
		("ulUnicodeRange2", 0),
		("ulUnicodeRange3", 0),
		("ulUnicodeRange4", 0),
		("fsSelection", 0),
		("usFirstCharIndex", 0),
		("usLastCharIndex", 0),
		("sTypoAscender", 0),
		("sTypoDescender", 0),
		("sTypoLineGap", 0),
		("usWinAscent", 0),
		("usWinDescent", 0),
		("ulCodePageRange1", 0),
		("ulCodePageRange2", 0),
		("sxHeight", 0),
		("sCapHeight", 0),
		("usDefaultChar", 0),
		("usBreakChar", 32),
		("usMaxContext", 0),
		("usLowerOpticalPointSize", 0),
		("usUpperOpticalPointSize", 0),
	];
	let mut out: Vec<(&'static str, Value)> =
		ints.iter().map(|&(n, v)| (n, Value::Int(v))).collect();
	out.push(("panose", Value::Panose(Panose::default())));
	out.push(("achVendID", Value::Str("????".into())));
	out
}

/// The `'static` name of a DICT entry.
fn dict_name(kind: DictKind, cff2: bool, key: &str) -> Result<&'static str> {
	dict_order(kind, cff2)
		.into_iter()
		.find(|n| *n == key)
		.ok_or_else(|| Error::value(format!("unknown DICT entry {key:?}")))
}

fn font_matrix(scale: f64) -> CffValue {
	let zero = CffValue::Int(0);
	CffValue::List(vec![
		CffValue::Float(scale),
		zero.clone(),
		zero.clone(),
		CffValue::Float(scale),
		zero.clone(),
		zero,
	])
}

fn char_strings_of(
	map: IndexMap<String, T2CharString>,
	private: PrivateRef,
	cff2: bool,
) -> CharStrings {
	let mut cs = CharStrings {
		has_fd_array: cff2,
		..Default::default()
	};
	for (name, charstring) in map {
		cs.set(name, charstring, private);
	}
	cs
}

/// A `cmap` subtable of a format for a mapping from code points to glyph IDs
/// (`buildCmapSubTable`).
pub fn build_cmap_subtable(
	cmapping: &BTreeMap<u32, u32>,
	format: u16,
	platform_id: u16,
	plat_enc_id: u16,
) -> CmapSubtable {
	let mut sub = CmapSubtable::new(format);
	*sub.cmap_mut() = cmapping.clone();
	sub.platform_id = platform_id;
	sub.plat_enc_id = plat_enc_id;
	sub.language = Some(0);
	sub
}

/// Adds an `fvar` table with the axes and named instances, adding their names
/// to the `name` table (`addFvar`). Macintosh names are added if the `name`
/// table has any.
pub fn add_fvar(font: &mut Font, axes: &[FvarAxis], instances: &[FvarInstance]) -> Result<()> {
	if axes.is_empty() {
		return Err(Error::Assertion("axes".into()));
	}
	let mac_names = font
		.get::<Name>()?
		.is_some_and(|n| n.names.iter().any(|r| r.platform_id == 1));
	// If there are no Mac names, don't add any; that would be inconsistent
	// (fonttools#683).
	let mut platforms: Vec<(u16, u16, u16)> = vec![(3, 1, 0x409)];
	if mac_names {
		platforms.insert(0, (1, 0, 0));
	}
	let mut fvar = Fvar::default();
	for axis_def in axes {
		let mut axis = Axis {
			axis_tag: axis_def.tag.to_bytes(),
			min_value: axis_def.minimum,
			default_value: axis_def.default,
			max_value: axis_def.maximum,
			..Default::default()
		};
		if axis_def.hidden {
			axis.flags = 0x0001;
		}
		axis.axis_name_id =
			add_multilingual_name(font, &axis_def.name.languages(), None, true, mac_names, 0)?;
		fvar.axes.push(axis);
	}
	for instance in instances {
		let mut inst = NamedInstance {
			subfamily_name_id: add_multilingual_name(
				font,
				&instance.style_name.languages(),
				None,
				true,
				mac_names,
				0,
			)?,
			..Default::default()
		};
		if let Some(ps) = &instance.postscript_font_name {
			inst.postscript_name_id = font.require::<Name>()?.add_name(ps, &platforms, 255)?;
		}
		inst.coordinates = instance.location.iter().copied().collect();
		fvar.instances.push(inst);
	}
	font.insert(Box::new(fvar));
	Ok(())
}
