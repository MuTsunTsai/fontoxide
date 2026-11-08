//! Reading CFF font sets from TTX (`fromXML` and `xmlRead` of `cffLib`).
//!
//! As in fontTools, DICTs read from TTX use the CFF (version 1) entry order and
//! defaults, also in a `CFF2` table.

use fontoxide_misc::py::{latin1_encode, parse_float, parse_int};
use fontoxide_misc::text_tools::{Literal, safe_eval};
use fontoxide_misc::xml_reader::Element;

use super::charstring::{T2CharString, index_error};
use super::dict::{Conv, PRIVATE_DICT, TOP_DICT, Value, operator};
use super::font::{
	CffFontSet, CharStrings, Encoding, FdSelect, FontDict, PrivateDict, PrivateRef, TopDict,
};
use crate::generated::ot;
use crate::ot::table::{OtTable, XmlCtx};
use crate::{Error, Font, Result};

fn key_error(name: &str) -> Error {
	Error::ttx(format!("KeyError: {name:?}"))
}

fn attr<'a>(e: &'a Element, name: &str) -> Result<&'a str> {
	e.attr(name).ok_or_else(|| key_error(name))
}

/// `int(s)`.
fn py_int(s: &str) -> Result<i64> {
	parse_int(s)
		.and_then(|v| i64::try_from(v).ok())
		.ok_or_else(|| Error::value(format!("invalid literal for int() with base 10: {s:?}")))
}

/// `parseNum`: an `int`, or else a `float`.
fn parse_num(s: &str) -> Result<Value> {
	if let Some(v) = parse_int(s) {
		return i64::try_from(v)
			.map(Value::Int)
			.map_err(|_| Error::value("int too large"));
	}
	parse_float(s)
		.map(Value::Float)
		.ok_or_else(|| Error::value(format!("could not convert string to float: {s:?}")))
}

/// `safeEval` of a number.
fn eval_number(s: &str) -> Result<Value> {
	match safe_eval(s) {
		Some(Literal::Int(v)) => i64::try_from(v)
			.map(Value::Int)
			.map_err(|_| Error::value("int too large")),
		Some(Literal::Float(f)) => Ok(Value::Float(f)),
		Some(Literal::Bool(b)) => Ok(Value::Int(b as i64)),
		_ => Err(Error::value(format!("malformed number: {s:?}"))),
	}
}

fn eval_int(s: &str) -> Result<i64> {
	match eval_number(s)? {
		Value::Int(v) => Ok(v),
		_ => Err(Error::ttx(format!("TypeError: expected an int: {s:?}"))),
	}
}

/// `parseBlendList`.
fn parse_blend_list(e: &Element) -> Result<Value> {
	let mut lists = Vec::new();
	for child in e.elements() {
		let values = attr(child, "value")?
			.split_whitespace()
			.map(eval_number)
			.collect::<Result<Vec<_>>>()?;
		lists.push(Value::List(values));
	}
	if lists.len() == 1 {
		return Ok(lists.pop().unwrap_or(Value::List(Vec::new())));
	}
	Ok(Value::List(lists))
}

/// `xmlRead` of the plain converters.
fn read_plain(conv: Conv, e: &Element) -> Result<Value> {
	Ok(match conv {
		Conv::Number => match e.attr("value") {
			Some(v) => parse_num(v)?,
			None => parse_blend_list(e)?,
		},
		Conv::Array => match e.attr("value") {
			Some(v) => Value::List(v.split_whitespace().map(parse_num).collect::<Result<_>>()?),
			None => parse_blend_list(e)?,
		},
		Conv::Ascii => {
			let v = attr(e, "value")?;
			if !v.is_ascii() {
				return Err(Error::ttx(
					"UnicodeEncodeError: 'ascii' codec can't encode character",
				));
			}
			Value::Str(v.to_owned())
		}
		Conv::Latin1 => {
			let v = attr(e, "value")?;
			if latin1_encode(v).is_none() {
				return Err(Error::ttx(
					"UnicodeEncodeError: 'latin-1' codec can't encode character",
				));
			}
			Value::Str(v.to_owned())
		}
		Conv::Ros => Value::Tuple(vec![
			Value::Str(attr(e, "Registry")?.to_owned()),
			Value::Str(attr(e, "Order")?.to_owned()),
			eval_number(attr(e, "Supplement")?)?,
		]),
		_ => {
			return Err(Error::ttx(
				"AttributeError: 'NoneType' object has no attribute 'xmlRead'",
			));
		}
	})
}

/// `SubrsIndex.fromXML` / the GlobalSubrs of `CFFFontSet.fromXML`.
fn read_subrs(e: &Element, only_char_strings: bool) -> Result<Vec<T2CharString>> {
	let mut subrs = Vec::new();
	for child in e.elements() {
		if only_char_strings && child.name != "CharString" {
			continue;
		}
		subrs.push(T2CharString::from_xml(child)?);
	}
	Ok(subrs)
}

fn read_private(e: &Element) -> Result<PrivateDict> {
	let mut private = PrivateDict::default();
	for child in e.elements() {
		let name = child.name.as_str();
		let op = operator(PRIVATE_DICT, name).ok_or_else(|| key_error(name))?;
		match op.conv {
			Conv::Subrs => private.subrs = Some(read_subrs(child, true)?),
			conv => {
				let value = read_plain(conv, child)?;
				private.values.insert(op.name, value);
			}
		}
	}
	Ok(private)
}

fn read_font_dict(e: &Element) -> Result<FontDict> {
	let mut fd = FontDict::default();
	for child in e.elements() {
		let name = child.name.as_str();
		let op = operator(TOP_DICT, name).ok_or_else(|| key_error(name))?;
		match op.conv {
			Conv::Private => fd.private = Some(Box::new(read_private(child)?)),
			Conv::Number | Conv::Array | Conv::Ascii | Conv::Latin1 | Conv::Ros => {
				let value = read_plain(op.conv, child)?;
				fd.values.insert(op.name, value);
			}
			// Only plain values and the Private DICT matter in a font DICT.
			_ => {
				return Err(Error::ttx(format!(
					"reading '{name}' in a CFF font DICT from TTX is not supported"
				)));
			}
		}
	}
	Ok(fd)
}

fn read_encoding(e: &Element) -> Result<Encoding> {
	if let Some(name) = e.attr("name") {
		return Ok(match name {
			"StandardEncoding" => Encoding::Standard,
			"ExpertEncoding" => Encoding::Expert,
			n => Encoding::Named(n.to_owned()),
		});
	}
	let mut encoding = vec![".notdef".to_owned(); 256];
	for child in e.elements() {
		let code = eval_int(attr(child, "code")?)?;
		let at = if code < 0 { code + 256 } else { code };
		let slot = usize::try_from(at)
			.ok()
			.and_then(|i| encoding.get_mut(i))
			.ok_or_else(|| Error::ttx("IndexError: list assignment index out of range"))?;
		*slot = attr(child, "name")?.to_owned();
	}
	Ok(Encoding::Custom(encoding))
}

/// `CharStringsConverter.xmlRead` and `CharStrings.fromXML`.
fn read_char_strings(e: &Element, top: &TopDict) -> Result<CharStrings> {
	let fds = top.fd_array.as_deref();
	if fds.is_none() && top.private.is_none() {
		return Err(Error::ttx("AttributeError: Private"));
	}
	let mut cs = CharStrings {
		has_fd_array: fds.is_some(),
		has_fd_select: top.fd_select.is_some(),
		..Default::default()
	};
	for child in e.elements() {
		if child.name != "CharString" {
			continue;
		}
		let (private, fd_id) = match fds {
			Some(fds) => {
				let id = match child.attr("fdSelectIndex") {
					Some(v) => eval_int(v)?,
					None => 0,
				};
				let at = if id < 0 { id + fds.len() as i64 } else { id };
				let at = usize::try_from(at)
					.ok()
					.filter(|&a| a < fds.len())
					.ok_or_else(index_error)?;
				if fds[at].private.is_none() {
					return Err(Error::ttx("AttributeError: Private"));
				}
				(PrivateRef::Fd(at), (id >= 0).then_some(id))
			}
			None => (PrivateRef::Top, None),
		};
		let name = attr(child, "name")?.to_owned();
		let mut charstring = T2CharString::from_xml(child)?;
		charstring.fd_select_index = fd_id;
		cs.set(name, charstring, private);
	}
	Ok(cs)
}

fn read_top_dict(top: &mut TopDict, e: &Element) -> Result<()> {
	let name = e.name.as_str();
	let op = operator(TOP_DICT, name).ok_or_else(|| key_error(name))?;
	match op.conv {
		Conv::Charset => top.charset = None,
		Conv::Encoding => top.encoding = Some(read_encoding(e)?),
		Conv::Private => top.private = Some(Box::new(read_private(e)?)),
		Conv::FdSelect => {
			let gid_array = match e.attr("gidArray") {
				Some(v) => v
					.split_whitespace()
					.map(|s| py_int(s).map(Some))
					.collect::<Result<_>>()?,
				None => Vec::new(),
			};
			top.fd_select = Some(FdSelect {
				format: Some(eval_int(attr(e, "format")?)?),
				gid_array,
				num_char_strings: None,
			});
		}
		Conv::FdArray => {
			let mut fds = Vec::new();
			for child in e.elements() {
				if child.name == "FontDict" {
					fds.push(read_font_dict(child)?);
				}
			}
			top.fd_array = Some(fds);
		}
		Conv::CharStrings => top.char_strings = Some(read_char_strings(e, top)?),
		Conv::VarStore => {
			let mut store = ot::VarStore::default();
			let mut dummy = Font::new();
			let mut ctx = XmlCtx::default();
			for child in e.elements() {
				store.from_xml(child, &mut dummy, &mut ctx)?;
			}
			top.var_store = Some(Box::new(store));
		}
		conv => {
			let value = read_plain(conv, e)?;
			top.values.insert(op.name, value);
		}
	}
	Ok(())
}

impl CffFontSet {
	/// `CFFFontSet.fromXML`: one element of the table.
	pub fn from_xml(&mut self, e: &Element) -> Result<()> {
		if !self.read_from_xml {
			// Defaults, replaced if there are entries for them.
			self.read_from_xml = true;
			self.major = 1;
			self.minor = 0;
		}
		match e.name.as_str() {
			"CFFFont" => {
				let font_name = attr(e, "name");
				match self.major {
					1 => {
						if !self.has_font_names {
							self.off_size = 4;
							self.hdr_size = 4;
							self.has_font_names = true;
							self.font_names.clear();
							self.top_dicts.clear();
						}
						self.font_names.push(font_name?.to_owned());
					}
					2 => {
						self.hdr_size = 5;
						if !self.has_font_names {
							self.has_font_names = true;
							self.font_names = vec!["CFF2Font".to_owned()];
						}
						self.top_dicts.clear();
					}
					_ => {
						return Err(Error::ttx(
							"UnboundLocalError: cannot access local variable 'topDict'",
						));
					}
				}
				let mut top = TopDict::default();
				for child in e.elements() {
					read_top_dict(&mut top, child)?;
				}
				if top.var_store.is_some() {
					let fds = top
						.fd_array
						.as_deref()
						.ok_or_else(|| Error::ttx("AttributeError: FDArray"))?;
					if fds.is_empty() {
						return Err(index_error());
					}
				}
				self.top_dicts.push(top);
			}
			"GlobalSubrs" => self.global_subrs.extend(read_subrs(e, false)?),
			"major" => self.major = py_int(attr(e, "value")?)?,
			"minor" => self.minor = py_int(attr(e, "value")?)?,
			_ => {}
		}
		Ok(())
	}
}
