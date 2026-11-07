//! `name`: Naming table (`fontTools.ttLib.tables._n_a_m_e`).

use std::cmp::Ordering;
use std::collections::HashMap;

use fontoxide_encodings::codecs;
use fontoxide_misc::Tag;
use fontoxide_misc::encoding_tools::get_encoding;
use fontoxide_misc::xml_reader::Element;
use fontoxide_misc::xml_writer::XmlWriter;

use crate::binary::{Reader, WriteBytes};
use crate::sstruct::{U16, eval};
use crate::table::{Table, TableType};
use crate::{Error, Font, Result};

crate::sstruct! {
	/// A name record as stored (`nameRecordFormat`).
	pub struct NameRecordFormat = "nameRecordFormat" {
		/// Platform ID.
		pub platform_id: u16 = "platformID" as U16,
		/// Platform-specific encoding ID.
		pub plat_enc_id: u16 = "platEncID" as U16,
		/// Language ID.
		pub lang_id: u16 = "langID" as U16,
		/// Name ID.
		pub name_id: u16 = "nameID" as U16,
		/// String length in bytes.
		pub length: u16 = "length" as U16,
		/// String offset from the start of the string storage.
		pub offset: u16 = "offset" as U16,
	}
}

/// The string of a name record: raw bytes as read from a font, or text set by the
/// user (fontTools allows both `bytes` and `str`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameString {
	/// Encoded bytes.
	Bytes(Vec<u8>),
	/// Text, encoded with the record's encoding when compiling.
	Text(String),
}

impl Default for NameString {
	fn default() -> Self {
		NameString::Bytes(Vec::new())
	}
}

impl From<&str> for NameString {
	fn from(s: &str) -> Self {
		NameString::Text(s.to_owned())
	}
}

impl From<String> for NameString {
	fn from(s: String) -> Self {
		NameString::Text(s)
	}
}

impl From<Vec<u8>> for NameString {
	fn from(b: Vec<u8>) -> Self {
		NameString::Bytes(b)
	}
}

/// A name record (`NameRecord`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NameRecord {
	/// Name ID.
	pub name_id: u16,
	/// Platform ID.
	pub platform_id: u16,
	/// Platform-specific encoding ID.
	pub plat_enc_id: u16,
	/// Language ID.
	pub lang_id: u16,
	/// The string.
	pub string: NameString,
}

fn is_ascii_char(b: u32) -> bool {
	(0x20..=0x7E).contains(&b) || matches!(b, 0x09 | 0x0A | 0x0D)
}

impl NameRecord {
	/// A record with the given string and IDs (`makeName`).
	pub fn new(
		string: impl Into<NameString>,
		name_id: u16,
		platform_id: u16,
		plat_enc_id: u16,
		lang_id: u16,
	) -> Self {
		NameRecord {
			name_id,
			platform_id,
			plat_enc_id,
			lang_id,
			string: string.into(),
		}
	}

	/// The codec for this record, or `"ascii"` if unknown (`getEncoding()`).
	pub fn encoding(&self) -> &'static str {
		get_encoding(self.platform_id, self.plat_enc_id, self.lang_id).unwrap_or("ascii")
	}

	/// Whether the encoding can represent any text (`encodingIsUnicodeCompatible`).
	pub fn encoding_is_unicode_compatible(&self) -> bool {
		matches!(
			get_encoding(self.platform_id, self.plat_enc_id, self.lang_id),
			Some("utf_16_be" | "ucs2be" | "ascii" | "latin1")
		)
	}

	/// Whether the string is Unicode (`isUnicode`).
	pub fn is_unicode(&self) -> bool {
		self.platform_id == 0 || (self.platform_id == 3 && matches!(self.plat_enc_id, 0 | 1 | 10))
	}

	/// The string as text (`toUnicode()`), with fontTools' heuristics to recover
	/// misencoded UTF-16.
	pub fn to_unicode(&self) -> std::result::Result<String, codecs::CodecError> {
		let encoding = self.encoding();
		let mut string = match &self.string {
			NameString::Text(s) => s.clone(),
			NameString::Bytes(bytes) => {
				let mut bytes = bytes.clone();
				if encoding == "utf_16_be" && bytes.len() % 2 == 1 {
					// Recover badly encoded UTF-16 strings that have an odd number
					// of bytes:
					// - If the last byte is zero, drop it. Otherwise,
					// - If all the odd bytes are zero and all the even bytes are
					//   ASCII, prepend one zero byte. Otherwise,
					// - If the first byte is zero and all other bytes are ASCII,
					//   insert zero bytes between consecutive ASCII bytes.
					if bytes.last() == Some(&0) {
						bytes.pop();
					} else if bytes.iter().enumerate().all(|(i, &b)| {
						if i % 2 == 1 {
							b == 0
						} else {
							is_ascii_char(b as u32)
						}
					}) {
						bytes.insert(0, 0);
					} else if bytes[0] == 0 && bytes[1..].iter().all(|&b| is_ascii_char(b as u32)) {
						bytes = bytes[1..].iter().flat_map(|&b| [0, b]).collect();
					}
				}
				codecs::decode(encoding, &bytes)?
			}
		};
		// If the decoded string still looks like UTF-16BE, it suggests a double
		// encoding; narrow it down.
		if string.chars().enumerate().all(|(i, c)| {
			if i % 2 == 0 {
				c == '\0'
			} else {
				is_ascii_char(c as u32)
			}
		}) {
			string = string.chars().skip(1).step_by(2).collect();
		}
		Ok(string)
	}

	/// The encoded string (`toBytes()`).
	pub fn to_bytes(&self) -> std::result::Result<Vec<u8>, codecs::CodecError> {
		match &self.string {
			NameString::Bytes(b) => Ok(b.clone()),
			NameString::Text(s) => codecs::encode(self.encoding(), s),
		}
	}

	/// The string with undecodable bytes escaped (`str(record)`).
	pub fn to_str_lossy(&self) -> String {
		match self.to_unicode() {
			Ok(s) => s,
			Err(_) => match &self.string {
				NameString::Bytes(b) => b
					.iter()
					.map(|&c| {
						if c < 0x80 {
							(c as char).to_string()
						} else {
							format!("\\x{c:02x}")
						}
					})
					.collect(),
				NameString::Text(s) => s.clone(),
			},
		}
	}

	fn ids(&self) -> (u16, u16, u16, u16) {
		(
			self.platform_id,
			self.plat_enc_id,
			self.lang_id,
			self.name_id,
		)
	}

	fn to_xml(&self, writer: &mut XmlWriter) {
		let unistr = self.to_unicode().ok();
		let lang_id = format!("{:#x}", self.lang_id);
		let unicode = unistr.is_some();
		let mut attrs: Vec<(&str, &dyn fontoxide_misc::xml_writer::AttrValue)> = vec![
			("nameID", &self.name_id),
			("platformID", &self.platform_id),
			("platEncID", &self.plat_enc_id),
			("langID", &lang_id),
		];
		if unistr.is_none() || !self.encoding_is_unicode_compatible() {
			attrs.push(("unicode", &unicode));
		}
		writer.begintag_ordered("namerecord", &attrs);
		writer.newline();
		match (&unistr, &self.string) {
			(Some(s), _) => writer.write(s),
			(None, NameString::Bytes(b)) => writer.write8bit(b, false),
			(None, NameString::Text(s)) => writer.write(s),
		}
		writer.newline();
		writer.endtag("namerecord");
		writer.newline();
	}

	fn from_xml(element: &Element) -> Result<Self> {
		let attr = |name: &str| {
			element
				.attr(name)
				.ok_or_else(|| Error::ttx(format!("missing '{name}' on <namerecord>")))
		};
		let int = |name: &str| -> Result<u16> {
			let v = eval(attr(name)?)?
				.as_int()
				.ok_or_else(|| Error::value(format!("invalid {name}")))?;
			crate::binary::fit(v, name)
		};
		let mut record = NameRecord {
			name_id: int("nameID")?,
			platform_id: int("platformID")?,
			plat_enc_id: int("platEncID")?,
			lang_id: int("langID")?,
			string: NameString::default(),
		};
		let text = element.text();
		let s = fontoxide_misc::py::strip(&text);
		let unicode = element
			.attr("unicode")
			.map(eval)
			.transpose()?
			.is_some_and(|v| v.is_truthy());
		let bytes = if record.encoding_is_unicode_compatible() || unicode {
			codecs::encode(record.encoding(), s).map_err(|e| Error::value(e.to_string()))?
		} else {
			// The inverse of write8bit.
			fontoxide_misc::py::latin1_encode(s)
				.ok_or_else(|| Error::value("'latin-1' codec can't encode the name record"))?
		};
		record.string = NameString::Bytes(bytes);
		Ok(record)
	}
}

/// `NameRecord.__lt__`, which compares the IDs and, if both strings can be
/// encoded, the encoded strings.
fn record_cmp(a: &(NameRecord, Option<Vec<u8>>), b: &(NameRecord, Option<Vec<u8>>)) -> Ordering {
	let ids = a.0.ids().cmp(&b.0.ids());
	match (&a.1, &b.1) {
		(Some(x), Some(y)) => ids.then_with(|| x.cmp(y)),
		_ => ids,
	}
}

/// A stable merge sort that, unlike `slice::sort_by`, tolerates the inconsistent
/// comparisons `record_cmp` can produce when some strings fail to encode.
fn merge_sort<T: Clone>(items: &mut [T], cmp: &impl Fn(&T, &T) -> Ordering) {
	if items.len() <= 1 {
		return;
	}
	let mid = items.len() / 2;
	merge_sort(&mut items[..mid], cmp);
	merge_sort(&mut items[mid..], cmp);
	let mut merged = Vec::with_capacity(items.len());
	let (mut i, mut j) = (0, mid);
	while i < mid && j < items.len() {
		if cmp(&items[j], &items[i]) == Ordering::Less {
			merged.push(items[j].clone());
			j += 1;
		} else {
			merged.push(items[i].clone());
			i += 1;
		}
	}
	merged.extend_from_slice(&items[i..mid]);
	merged.extend_from_slice(&items[j..]);
	items.clone_from_slice(&merged);
}

/// Naming table (`table__n_a_m_e`).
///
/// See <https://learn.microsoft.com/en-us/typography/opentype/spec/name>.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Name {
	/// The name records (`names`).
	pub names: Vec<NameRecord>,
}

impl Name {
	/// The first record with the given IDs; any language if `lang_id` is `None`
	/// (`getName`).
	pub fn get_name(
		&self,
		name_id: u16,
		platform_id: u16,
		plat_enc_id: u16,
		lang_id: Option<u16>,
	) -> Option<&NameRecord> {
		self.names.iter().find(|r| {
			r.name_id == name_id
				&& r.platform_id == platform_id
				&& r.plat_enc_id == plat_enc_id
				&& lang_id.is_none_or(|l| r.lang_id == l)
		})
	}

	/// A name for debugging or display, preferring Windows English, then Mac
	/// English, then any decodable record (`getDebugName`).
	pub fn get_debug_name(&self, name_id: u16) -> Option<String> {
		let mut mac_english = None;
		let mut some = None;
		for record in self.names.iter().filter(|r| r.name_id == name_id) {
			let Ok(s) = record.to_unicode() else {
				continue;
			};
			if (record.platform_id, record.lang_id) == (3, 0x409) {
				if !s.is_empty() {
					return Some(s);
				}
				some = Some(s);
				break;
			}
			if (record.platform_id, record.lang_id) == (1, 0) && mac_english.is_none() {
				mac_english = Some(s.clone());
			}
			some = Some(s);
		}
		// fontTools tests the candidates for truthiness, so empty strings are
		// skipped except as the last resort.
		mac_english
			.filter(|s| !s.is_empty())
			.or_else(|| some.filter(|s| !s.is_empty()))
	}

	/// The first name found for the given IDs (`getFirstDebugName`).
	pub fn get_first_debug_name(&self, name_ids: &[u16]) -> Option<String> {
		name_ids.iter().find_map(|&id| self.get_debug_name(id))
	}

	/// WWS, typographic or legacy family name (`getBestFamilyName`).
	pub fn best_family_name(&self) -> Option<String> {
		self.get_first_debug_name(&[21, 16, 1])
	}

	/// WWS, typographic or legacy subfamily name (`getBestSubFamilyName`).
	pub fn best_sub_family_name(&self) -> Option<String> {
		self.get_first_debug_name(&[22, 17, 2])
	}

	/// The full name, from the family and subfamily names or name IDs 4 or 6
	/// (`getBestFullName`).
	pub fn best_full_name(&self) -> Option<String> {
		for (family, sub) in [(21, 22), (16, 17), (1, 2)] {
			let (Some(fam), Some(subfam)) = (self.get_debug_name(family), self.get_debug_name(sub))
			else {
				continue;
			};
			return Some(if subfam.to_lowercase() == "regular" {
				fam
			} else {
				format!("{fam} {subfam}")
			});
		}
		self.get_debug_name(4).or_else(|| self.get_debug_name(6))
	}

	/// Sets the string of a record, adding the record if needed (`setName`).
	pub fn set_name(
		&mut self,
		string: impl Into<NameString>,
		name_id: u16,
		platform_id: u16,
		plat_enc_id: u16,
		lang_id: u16,
	) {
		let string = string.into();
		if let NameString::Bytes(b) = &string {
			log::warn!(
				"name string is bytes, ensure it's correctly encoded: {}",
				fontoxide_misc::py::bytes_repr(b)
			);
		}
		match self.names.iter_mut().find(|r| {
			r.name_id == name_id
				&& r.platform_id == platform_id
				&& r.plat_enc_id == plat_enc_id
				&& r.lang_id == lang_id
		}) {
			Some(r) => r.string = string,
			None => self.names.push(NameRecord::new(
				string,
				name_id,
				platform_id,
				plat_enc_id,
				lang_id,
			)),
		}
	}

	/// Removes the records matching all the given IDs (`removeNames`). Without any
	/// ID, nothing is removed.
	pub fn remove_names(
		&mut self,
		name_id: Option<u16>,
		platform_id: Option<u16>,
		plat_enc_id: Option<u16>,
		lang_id: Option<u16>,
	) {
		if name_id.is_none() && platform_id.is_none() && plat_enc_id.is_none() && lang_id.is_none()
		{
			return;
		}
		self.names.retain(|r| {
			name_id.is_some_and(|v| v != r.name_id)
				|| platform_id.is_some_and(|v| v != r.platform_id)
				|| plat_enc_id.is_some_and(|v| v != r.plat_enc_id)
				|| lang_id.is_some_and(|v| v != r.lang_id)
		});
	}

	/// An unused name ID after the highest one in the table, at least
	/// `min_name_id` (`_findUnusedNameID`).
	pub fn find_unused_name_id(&self, min_name_id: u16) -> Result<u16> {
		let name_id = 1 + self
			.names
			.iter()
			.map(|n| n.name_id as i64)
			.chain([min_name_id as i64 - 1])
			.max()
			.unwrap_or(-1);
		if name_id > 32767 {
			return Err(Error::value("nameID must be less than 32768"));
		}
		Ok(name_id as u16)
	}

	/// Adds records for `string` on each `(platformID, platEncID, langID)` and
	/// returns the new name ID, which is above `min_name_id` (`addName`).
	pub fn add_name(
		&mut self,
		string: &str,
		platforms: &[(u16, u16, u16)],
		min_name_id: u16,
	) -> Result<u16> {
		if platforms.is_empty() {
			return Err(Error::Assertion(
				"'platforms' must contain at least one (platformID, platEncID, langID) tuple"
					.into(),
			));
		}
		let name_id = self.find_unused_name_id(min_name_id.saturating_add(1))?;
		for &(p, e, l) in platforms {
			self.names.push(NameRecord::new(string, name_id, p, e, l));
		}
		Ok(name_id)
	}

	/// [`Name::add_name`] with fontTools' default platforms: Macintosh English
	/// and Windows English (US), and a minimum name ID of 255.
	pub fn add_name_default(&mut self, string: &str) -> Result<u16> {
		self.add_name(string, &[(1, 0, 0), (3, 1, 0x409)], 255)
	}

	/// Sorts the records as the specification requires (`names.sort()`).
	pub fn sort(&mut self) {
		let mut keyed: Vec<(NameRecord, Option<Vec<u8>>)> = self
			.names
			.drain(..)
			.map(|r| {
				let b = r.to_bytes();
				if let Err(e) = &b {
					log::error!("NameRecord sorting failed to encode: {e}");
				}
				let b = b.ok();
				(r, b)
			})
			.collect();
		merge_sort(&mut keyed, &record_cmp);
		self.names = keyed.into_iter().map(|(r, _)| r).collect();
	}
}

impl TableType for Name {
	const TAG: Tag = Tag::new(b"name");

	fn decompile(data: &[u8], _font: &mut Font) -> Result<Self> {
		let mut r = Reader::new(data);
		let _format = r.u16()?;
		let n = r.u16()? as usize;
		let string_offset = r.u16()? as usize;
		let expected = 6 + n * NameRecordFormat::SIZE;
		if string_offset != expected {
			log::error!(
				"'name' table stringOffset incorrect. Expected: {expected}; Actual: {string_offset}"
			);
		}
		let string_data = data.get(string_offset..).unwrap_or_default();
		let mut records = &data[6..];
		let mut names = Vec::with_capacity(n);
		for i in 0..n {
			if records.len() < 12 {
				log::error!("skipping malformed name record #{i}");
				continue;
			}
			let (rec, rest) = NameRecordFormat::unpack2(records)?;
			records = rest;
			let start = rec.offset as usize;
			let end = start + rec.length as usize;
			if end > string_data.len() {
				log::error!("skipping malformed name record #{i}");
				continue;
			}
			names.push(NameRecord {
				name_id: rec.name_id,
				platform_id: rec.platform_id,
				plat_enc_id: rec.plat_enc_id,
				lang_id: rec.lang_id,
				string: NameString::Bytes(string_data[start..end].to_vec()),
			});
		}
		Ok(Name { names })
	}
}

impl Table for Name {
	fn tag(&self) -> Tag {
		Self::TAG
	}

	fn compile(&mut self, _font: &mut Font) -> Result<Vec<u8>> {
		self.sort();
		let n = self.names.len();
		let string_offset = 6 + n * NameRecordFormat::SIZE;
		let mut data = Vec::with_capacity(string_offset);
		data.put_u16(0);
		data.put_u16(crate::binary::fit(n, "count")?);
		data.put_u16(crate::binary::fit(string_offset, "stringOffset")?);
		let mut string_data: Vec<u8> = Vec::new();
		let mut done: HashMap<Vec<u8>, (usize, usize)> = HashMap::new();
		for name in &self.names {
			let string = name.to_bytes().map_err(|e| Error::value(e.to_string()))?;
			let (offset, length) = match done.get(&string) {
				Some(&v) => v,
				None => {
					let v = (string_data.len(), string.len());
					string_data.extend_from_slice(&string);
					done.insert(string, v);
					v
				}
			};
			NameRecordFormat {
				platform_id: name.platform_id,
				plat_enc_id: name.plat_enc_id,
				lang_id: name.lang_id,
				name_id: name.name_id,
				length: crate::binary::fit(length, "length")?,
				offset: crate::binary::fit(offset, "offset")?,
			}
			.pack(&mut data)?;
		}
		data.extend(string_data);
		Ok(data)
	}

	fn to_xml(&self, writer: &mut XmlWriter, _font: &mut Font) -> Result<()> {
		for name in &self.names {
			name.to_xml(writer);
		}
		Ok(())
	}

	fn from_xml(&mut self, element: &Element, _font: &mut Font) -> Result<()> {
		if element.name != "namerecord" {
			return Ok(());
		}
		self.names.push(NameRecord::from_xml(element)?);
		Ok(())
	}

	fn clone_table(&self) -> Box<dyn Table> {
		Box::new(self.clone())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn format_matches_upstream() {
		crate::sstruct::tests::check(NameRecordFormat::FORMAT_NAME, NameRecordFormat::FIELDS);
	}
}
