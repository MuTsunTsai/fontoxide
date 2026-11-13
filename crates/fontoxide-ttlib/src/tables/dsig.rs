//! `DSIG`: Digital Signature table (`fontTools.ttLib.tables.D_S_I_G_`).

use fontoxide_misc::Tag;
use fontoxide_misc::py;
use fontoxide_misc::xml_reader::{Content, Element};
use fontoxide_misc::xml_writer::XmlWriter;

use crate::binary::{Reader, WriteBytes};
use crate::table::{Table, TableType, TableXml};
use crate::tables::eblc::{clip, int_attr};
use crate::{Error, Font, Result};

const HEADER_SIZE: u64 = 8;
const SIGNATURE_SIZE: u64 = 12;
const BLOCK_SIZE: u64 = 8;

/// A signature (`SignatureRecord`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SignatureRecord {
	/// `ulFormat`.
	pub format: u32,
	/// `ulLength`, updated by compiling.
	pub length: u32,
	/// `ulOffset`, updated by compiling.
	pub offset: u32,
	/// `usReserved1`.
	pub reserved1: u16,
	/// `usReserved2`.
	pub reserved2: u16,
	/// The raw PKCS #7 data (`pkcs7`).
	pub pkcs7: Vec<u8>,
}

/// Digital Signature table (`table_D_S_I_G_`).
///
/// See <https://learn.microsoft.com/en-us/typography/opentype/spec/dsig>.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Dsig {
	/// `ulVersion`.
	pub version: u32,
	/// `usNumSigs`, which is written as is.
	pub num_sigs: u16,
	/// `usFlag`.
	pub flag: u16,
	/// The signatures (`signatureRecords`), if set.
	pub signature_records: Option<Vec<SignatureRecord>>,
}

impl Dsig {
	fn records(&self) -> Result<&Vec<SignatureRecord>> {
		self.signature_records.as_ref().ok_or_else(no_records)
	}
}

fn no_records() -> Error {
	Error::ttlib("AttributeError: 'table_D_S_I_G_' object has no attribute 'signatureRecords'")
}

/// A reader over exactly the first `n` bytes, as `sstruct.unpack2` requires.
fn fixed(data: &[u8], n: u64) -> Result<Reader<'_>> {
	let slice = clip(data, 0, n);
	if (slice.len() as u64) < n {
		return Err(Error::Struct(format!(
			"unpack requires a buffer of {n} bytes"
		)));
	}
	Ok(Reader::new(slice))
}

impl TableType for Dsig {
	const TAG: Tag = Tag::new(b"DSIG");

	fn decompile(data: &[u8], _font: &mut Font) -> Result<Self> {
		let mut r = fixed(data, HEADER_SIZE)?;
		let version = r.u32()?;
		let num_sigs = r.u16()?;
		let flag = r.u16()?;
		if version != 1 {
			return Err(Error::Assertion("DSIG ulVersion must be 1".into()));
		}
		if flag & !1 != 0 {
			return Err(Error::Assertion("DSIG usFlag must be 0x1 or 0x0".into()));
		}
		let mut records = Vec::with_capacity(usize::from(num_sigs));
		let mut rest = clip(data, HEADER_SIZE, u64::MAX);
		for n in 0..num_sigs {
			let mut r = fixed(rest, SIGNATURE_SIZE)?;
			let format = r.u32()?;
			let length = r.u32()?;
			let offset = r.u32()?;
			rest = clip(rest, SIGNATURE_SIZE, u64::MAX);
			if format != 1 {
				return Err(Error::Assertion(format!(
					"DSIG signature record #{n} ulFormat must be 1"
				)));
			}
			records.push(SignatureRecord {
				format,
				length,
				offset,
				..Default::default()
			});
		}
		// The message of fontTools names the last record, whichever one fails.
		let last = usize::from(num_sigs).saturating_sub(1);
		for record in &mut records {
			let block = clip(data, u64::from(record.offset), u64::MAX);
			let mut r = fixed(block, BLOCK_SIZE)?;
			record.reserved1 = r.u16()?;
			record.reserved2 = r.u16()?;
			let cb_signature = r.i32()?;
			if record.reserved1 != 0 {
				return Err(Error::Assertion(format!(
					"DSIG signature record #{last} usReserverd1 must be 0"
				)));
			}
			if record.reserved2 != 0 {
				return Err(Error::Assertion(format!(
					"DSIG signature record #{last} usReserverd2 must be 0"
				)));
			}
			let rest = clip(block, BLOCK_SIZE, u64::MAX);
			// `newData[:cbSignature]`: the count may be negative.
			let end = if cb_signature < 0 {
				(rest.len() as u64).saturating_sub(u64::from(cb_signature.unsigned_abs()))
			} else {
				cb_signature as u64
			};
			record.pkcs7 = clip(rest, 0, end).to_vec();
		}
		Ok(Dsig {
			version,
			num_sigs,
			flag,
			signature_records: Some(records),
		})
	}
}

impl Table for Dsig {
	fn tag(&self) -> Tag {
		Self::TAG
	}

	fn compile(&mut self, _font: &mut Font) -> Result<Vec<u8>> {
		let num_sigs = self.num_sigs;
		let mut headers = Vec::new();
		headers.put_u32(self.version);
		headers.put_u16(num_sigs);
		headers.put_u16(self.flag);
		let mut offset = HEADER_SIZE + u64::from(num_sigs) * SIGNATURE_SIZE;
		let mut data = Vec::new();
		let records = self.signature_records.as_mut().ok_or_else(no_records)?;
		for record in records {
			let packed = BLOCK_SIZE + record.pkcs7.len() as u64;
			data.put_u16(record.reserved1);
			data.put_u16(record.reserved2);
			data.put_i32(crate::binary::fit(record.pkcs7.len(), "cbSignature")?);
			data.extend_from_slice(&record.pkcs7);
			record.length = crate::binary::fit(packed, "ulLength")?;
			record.offset = crate::binary::fit(offset, "ulOffset")?;
			headers.put_u32(record.format);
			headers.put_u32(record.length);
			headers.put_u32(record.offset);
			offset += packed;
		}
		if !offset.is_multiple_of(2) {
			// Pad to even bytes.
			data.push(0);
		}
		headers.extend(data);
		Ok(headers)
	}

	fn clone_table(&self) -> Box<dyn Table> {
		Box::new(self.clone())
	}

	fn remap_glyphs(&mut self, _f: &mut dyn FnMut(u32) -> Result<u32>) -> Result<()> {
		Ok(())
	}
}

impl TableXml for Dsig {
	fn to_xml(&self, writer: &mut XmlWriter, _font: &mut Font) -> Result<()> {
		writer.comment("note that the Digital Signature will be invalid after recompilation!");
		writer.newline();
		let flag = format!("0x{:X}", self.flag);
		writer.simpletag(
			"tableHeader",
			&[
				("version", &self.version),
				("numSigs", &self.num_sigs),
				("flag", &flag),
			],
		);
		for record in self.records()? {
			writer.newline();
			writer.begintag("SignatureRecord", &[("format", &record.format)]);
			writer.newline();
			writer.write_noindent("-----BEGIN PKCS7-----\n");
			writer.write_noindent(&b64encode(&record.pkcs7));
			writer.write_noindent("-----END PKCS7-----\n");
			writer.endtag("SignatureRecord");
		}
		writer.newline();
		Ok(())
	}

	fn from_xml(&mut self, element: &Element, _font: &mut Font) -> Result<()> {
		match element.name.as_str() {
			"tableHeader" => {
				self.signature_records = Some(Vec::new());
				self.version = int_attr(element, "version")?;
				self.num_sigs = int_attr(element, "numSigs")?;
				self.flag = int_attr(element, "flag")?;
			}
			"SignatureRecord" => {
				let reserved = |name: &str| -> Result<u16> {
					if element.attr(name).is_some() {
						int_attr(element, name)
					} else {
						Ok(0)
					}
				};
				let record = SignatureRecord {
					format: int_attr(element, "format")?,
					reserved1: reserved("reserved1")?,
					reserved2: reserved("reserved2")?,
					pkcs7: pkcs7_from_content(element)?,
					..Default::default()
				};
				self.signature_records
					.as_mut()
					.ok_or_else(no_records)?
					.push(record);
			}
			_ => {}
		}
		Ok(())
	}
}

/// `base64.b64decode(tobytes(strjoin(filter(pem_spam, content))))`.
fn pkcs7_from_content(element: &Element) -> Result<Vec<u8>> {
	let mut text = String::new();
	for chunk in &element.content {
		let Content::Text(line) = chunk else {
			return Err(Error::ttlib(
				"AttributeError: 'tuple' object has no attribute 'strip'",
			));
		};
		// `pem_spam`.
		let stripped = py::strip(line);
		if !matches!(
			stripped,
			"-----BEGIN PKCS7-----" | "-----END PKCS7-----" | ""
		) {
			text.push_str(line);
		}
	}
	if !text.is_ascii() {
		return Err(Error::ttlib(
			"UnicodeEncodeError: 'ascii' codec can't encode characters",
		));
	}
	b64decode(text.as_bytes())
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `b64encode`: base64, with a line break after every 76 characters.
fn b64encode(data: &[u8]) -> String {
	let mut encoded = String::with_capacity(data.len().div_ceil(3) * 4);
	for chunk in data.chunks(3) {
		let b = [
			chunk[0],
			chunk.get(1).copied().unwrap_or(0),
			chunk.get(2).copied().unwrap_or(0),
		];
		let sextets = [
			b[0] >> 2,
			((b[0] & 3) << 4) | (b[1] >> 4),
			((b[1] & 15) << 2) | (b[2] >> 6),
			b[2] & 63,
		];
		for (i, &s) in sextets.iter().enumerate() {
			if i <= chunk.len() {
				encoded.push(char::from(B64[usize::from(s)]));
			} else {
				encoded.push('=');
			}
		}
	}
	let mut lines = String::with_capacity(encoded.len() + encoded.len() / 76 + 1);
	for line in encoded.as_bytes().chunks(76) {
		lines.extend(line.iter().map(|&b| char::from(b)));
		lines.push('\n');
	}
	lines
}

/// `binascii.a2b_base64` without `strict_mode`: other characters are skipped,
/// and a complete padding ends the input.
fn b64decode(text: &[u8]) -> Result<Vec<u8>> {
	let mut out = Vec::with_capacity(text.len() / 4 * 3);
	let mut quad_pos = 0;
	let mut left = 0u8;
	let mut pads = 0;
	let mut characters = 0usize;
	for &c in text {
		if c == b'=' {
			pads += 1;
			if quad_pos >= 2 && quad_pos + pads >= 4 {
				return Ok(out);
			}
			continue;
		}
		let Some(value) = B64.iter().position(|&b| b == c) else {
			continue;
		};
		let value = value as u8;
		pads = 0;
		characters += 1;
		match quad_pos {
			0 => {
				quad_pos = 1;
				left = value;
			}
			1 => {
				quad_pos = 2;
				out.push((left << 2) | (value >> 4));
				left = value & 0x0F;
			}
			2 => {
				quad_pos = 3;
				out.push((left << 4) | (value >> 2));
				left = value & 0x03;
			}
			_ => {
				quad_pos = 0;
				out.push((left << 6) | value);
				left = 0;
			}
		}
	}
	match quad_pos {
		0 => Ok(out),
		1 => Err(Error::value(format!(
			"Invalid base64-encoded string: number of data characters ({characters}) cannot be 1 more than a multiple of 4"
		))),
		_ => Err(Error::value("Incorrect padding")),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn base64_matches_python() -> Result<()> {
		assert_eq!(b64encode(b""), "");
		assert_eq!(b64encode(b"f"), "Zg==\n");
		assert_eq!(b64encode(b"fo"), "Zm8=\n");
		assert_eq!(b64encode(b"foo"), "Zm9v\n");
		let long = b64encode(&[0u8; 60]);
		assert_eq!(long.lines().next().map(str::len), Some(76));
		assert_eq!(long.lines().count(), 2);
		assert_eq!(b64decode(b"Zm 9\nv")?, b"foo");
		assert_eq!(b64decode(b"Zg==ignored")?, b"f");
		assert_eq!(b64decode(b"Zm8=")?, b"fo");
		assert!(b64decode(b"Z").is_err());
		assert!(b64decode(b"Zg").is_err());
		assert!(b64decode(b"Zg=").is_err());
		Ok(())
	}

	#[test]
	fn round_trip() -> Result<()> {
		let mut font = Font::new();
		let data = [
			&[0, 0, 0, 1, 0, 1, 0, 1][..],
			&[0, 0, 0, 1, 0, 0, 0, 11, 0, 0, 0, 20][..],
			&[0, 0, 0, 0, 0, 0, 0, 3, 1, 2, 3][..],
			&[0][..],
		]
		.concat();
		let mut table = Dsig::decompile(&data, &mut font)?;
		assert_eq!(table.records()?[0].pkcs7, [1, 2, 3]);
		assert_eq!(table.compile(&mut font)?, data);
		Ok(())
	}
}
