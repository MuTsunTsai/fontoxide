//! TTX output of CFF font sets (`toXML` of `cffLib`).

use std::borrow::Cow;

use fontoxide_misc::xml_writer::{AttrValue, XmlWriter};

use super::charstring::T2CharString;
use super::dict::{Conv, Value, operator};
use super::font::{
	CffFontSet, CharStrings, DictKind, Encoding, FdSelect, FontDict, PrivateDict, TopDict,
	dict_order, dict_table,
};
use crate::ot::table::OtTable;
use crate::{Font, Result};

impl AttrValue for Value {
	fn attr_string(&self) -> Cow<'_, str> {
		Cow::Owned(self.repr())
	}
}

/// `" ".join([str(val) for val in value])`.
fn join(values: &[Value]) -> String {
	values.iter().map(Value::repr).collect::<Vec<_>>().join(" ")
}

/// Python's `\s` on `str`: Unicode whitespace, with the information separators.
fn is_py_space(c: char) -> bool {
	c.is_whitespace() || ('\x1c'..='\x1f').contains(&c)
}

/// `re.sub(r"[\r\n]\s+", " ", value)`.
fn collapse_line_breaks(s: &str) -> String {
	let chars: Vec<char> = s.chars().collect();
	let mut out = String::with_capacity(s.len());
	let mut i = 0;
	while i < chars.len() {
		let c = chars[i];
		if (c == '\r' || c == '\n') && chars.get(i + 1).is_some_and(|&n| is_py_space(n)) {
			i += 1;
			while chars.get(i).is_some_and(|&n| is_py_space(n)) {
				i += 1;
			}
			out.push(' ');
			continue;
		}
		out.push(c);
		i += 1;
	}
	out
}

/// `xmlWrite` of the plain converters.
fn write_value(xw: &mut XmlWriter, conv: Conv, name: &str, value: &Value) {
	match (conv, value) {
		(Conv::Number, Value::List(items)) => {
			xw.begintag(name, &[]);
			xw.newline();
			xw.indent();
			xw.simpletag("blend", &[("value", &join(items))]);
			xw.newline();
			xw.dedent();
			xw.endtag(name);
			xw.newline();
		}
		(Conv::Array, Value::List(items)) if matches!(items.first(), Some(Value::List(_))) => {
			xw.begintag(name, &[]);
			xw.newline();
			xw.indent();
			for item in items {
				let text = match item {
					Value::List(l) => join(l),
					v => v.repr(),
				};
				xw.simpletag("blend", &[("value", &text)]);
				xw.newline();
			}
			xw.dedent();
			xw.endtag(name);
			xw.newline();
		}
		(Conv::Array, Value::List(items)) => {
			xw.simpletag(name, &[("value", &join(items))]);
			xw.newline();
		}
		(Conv::Latin1, v) if matches!(name, "Notice" | "Copyright") => {
			xw.simpletag(name, &[("value", &collapse_line_breaks(&v.repr()))]);
			xw.newline();
		}
		(Conv::Ros, Value::Tuple(t)) if t.len() == 3 => {
			xw.simpletag_ordered(
				name,
				&[
					("Registry", &t[0].repr()),
					("Order", &t[1].repr()),
					("Supplement", &t[2]),
				],
			);
			xw.newline();
		}
		(_, v) => {
			xw.simpletag(name, &[("value", v)]);
			xw.newline();
		}
	}
}

/// The comment on the keys outside `order` (`BaseDict.toXML`).
fn ignored_keys(xw: &mut XmlWriter, raw: &[(&'static str, Value)], order: &[&str]) {
	let mut ignored: Vec<&str> = raw
		.iter()
		.map(|(n, _)| *n)
		.filter(|n| !order.contains(n))
		.collect();
	ignored.sort_unstable();
	ignored.dedup();
	if !ignored.is_empty() {
		xw.comment(&format!("some keys were ignored: {}", ignored.join(" ")));
		xw.newline();
	}
}

/// `GlobalSubrsIndex.toXML`.
fn subrs_to_xml(xw: &mut XmlWriter, subrs: &[T2CharString]) -> Result<()> {
	xw.comment("The 'index' attribute is only for humans; it is ignored when parsed.");
	xw.newline();
	for (i, subr) in subrs.iter().enumerate() {
		if subr.needs_decompilation() {
			xw.begintag("CharString", &[("index", &i), ("raw", &1)]);
		} else {
			xw.begintag("CharString", &[("index", &i)]);
		}
		xw.newline();
		subr.to_xml(xw)?;
		xw.endtag("CharString");
		xw.newline();
	}
	Ok(())
}

fn private_to_xml(xw: &mut XmlWriter, private: &PrivateDict) -> Result<()> {
	let order = dict_order(DictKind::Private, private.cff2);
	let table = dict_table(DictKind::Private);
	for name in &order {
		if *name == "Subrs" {
			if let Some(subrs) = &private.subrs {
				xw.begintag("Subrs", &[]);
				xw.newline();
				subrs_to_xml(xw, subrs)?;
				xw.endtag("Subrs");
				xw.newline();
			}
			continue;
		}
		let Some(op) = operator(table, name) else {
			continue;
		};
		if op.conv == Conv::None {
			continue;
		}
		if let Some(value) = private.get(name) {
			write_value(xw, op.conv, name, &value);
		}
	}
	ignored_keys(xw, &private.raw, &order);
	Ok(())
}

fn write_private(xw: &mut XmlWriter, private: &PrivateDict) -> Result<()> {
	xw.begintag("Private", &[]);
	xw.newline();
	private_to_xml(xw, private)?;
	xw.endtag("Private");
	xw.newline();
	Ok(())
}

fn font_dict_to_xml(xw: &mut XmlWriter, fd: &FontDict) -> Result<()> {
	let order = dict_order(DictKind::Font, fd.cff2);
	let table = dict_table(DictKind::Font);
	for name in &order {
		if *name == "Private" {
			if let Some(p) = &fd.private {
				write_private(xw, p)?;
			}
			continue;
		}
		if let (Some(op), Some(value)) = (operator(table, name), fd.values.get(name)) {
			write_value(xw, op.conv, name, value);
		}
	}
	ignored_keys(xw, &fd.raw, &order);
	Ok(())
}

fn encoding_to_xml(xw: &mut XmlWriter, encoding: &Encoding) {
	let names: Vec<String> = match encoding {
		Encoding::Standard | Encoding::Expert => {
			let name = if *encoding == Encoding::Standard {
				"StandardEncoding"
			} else {
				"ExpertEncoding"
			};
			xw.simpletag("Encoding", &[("name", &name)]);
			xw.newline();
			return;
		}
		Encoding::Named(s) if s == "StandardEncoding" || s == "ExpertEncoding" => {
			xw.simpletag("Encoding", &[("name", s)]);
			xw.newline();
			return;
		}
		// A string is iterated by character.
		Encoding::Named(s) => s.chars().map(String::from).collect(),
		Encoding::Custom(names) => names.clone(),
	};
	xw.begintag("Encoding", &[]);
	xw.newline();
	for (code, name) in names.iter().enumerate() {
		if name != ".notdef" {
			xw.simpletag("map", &[("code", &format!("{code:#x}")), ("name", name)]);
			xw.newline();
		}
	}
	xw.endtag("Encoding");
	xw.newline();
}

fn fd_select_to_xml(xw: &mut XmlWriter, fd_select: &FdSelect) {
	let format = fd_select
		.format
		.map_or_else(|| "None".to_owned(), |f| f.to_string());
	let gids = fd_select.gid_array.len();
	if gids > fd_select.num_char_strings.unwrap_or(gids) {
		let array = fd_select
			.gid_array
			.iter()
			.map(|g| g.map_or_else(|| "None".to_owned(), |g| g.to_string()))
			.collect::<Vec<_>>()
			.join(" ");
		xw.simpletag_ordered("FDSelect", &[("format", &format), ("gidArray", &array)]);
	} else {
		xw.simpletag_ordered("FDSelect", &[("format", &format)]);
	}
	xw.newline();
}

fn char_strings_to_xml(xw: &mut XmlWriter, cs: &CharStrings) -> Result<()> {
	let mut order: Vec<usize> = (0..cs.names.len()).collect();
	order.sort_by(|&a, &b| cs.names[a].cmp(&cs.names[b]));
	for i in order {
		let name = &cs.names[i];
		let item = &cs.items[i];
		let selector = cs.selector(i)?;
		let raw = item.needs_decompilation();
		match (selector, raw) {
			(None, false) => xw.begintag_ordered("CharString", &[("name", name)]),
			(None, true) => xw.begintag_ordered("CharString", &[("name", name), ("raw", &1)]),
			(Some(fd), false) => {
				xw.begintag_ordered("CharString", &[("name", name), ("fdSelectIndex", &fd)])
			}
			(Some(fd), true) => xw.begintag_ordered(
				"CharString",
				&[("name", name), ("fdSelectIndex", &fd), ("raw", &1)],
			),
		}
		xw.newline();
		item.to_xml(xw)?;
		xw.endtag("CharString");
		xw.newline();
	}
	Ok(())
}

impl TopDict {
	/// `TopDict.toXML`, after the charstrings were decompiled.
	fn to_xml(&self, xw: &mut XmlWriter) -> Result<()> {
		let mut skip: &[&str] = &[];
		if self.is_cid() {
			skip = &["Encoding"];
		}
		if !self.is_cid() || self.char_strings.is_none() {
			skip = &[
				"CIDFontVersion",
				"CIDFontRevision",
				"CIDFontType",
				"CIDCount",
			];
		}
		let order = dict_order(DictKind::Top, self.cff2);
		let table = dict_table(DictKind::Top);
		for name in &order {
			if skip.contains(name) {
				continue;
			}
			let Some(op) = operator(table, name) else {
				continue;
			};
			match op.conv {
				Conv::Charset => {
					xw.comment("charset is dumped separately as the 'GlyphOrder' element");
					xw.newline();
				}
				Conv::Encoding => {
					if let Some(e) = &self.encoding {
						encoding_to_xml(xw, e);
					}
				}
				Conv::Private => {
					if let Some(p) = &self.private {
						write_private(xw, p)?;
					}
				}
				Conv::FdSelect => {
					if let Some(s) = &self.fd_select {
						let mut s = s.clone();
						if let Some(cs) = &self.char_strings {
							s.num_char_strings = Some(cs.items.len());
						}
						fd_select_to_xml(xw, &s);
					}
				}
				Conv::FdArray => {
					if let Some(fds) = &self.fd_array {
						xw.begintag("FDArray", &[]);
						xw.newline();
						for (i, fd) in fds.iter().enumerate() {
							xw.begintag("FontDict", &[("index", &i)]);
							xw.newline();
							font_dict_to_xml(xw, fd)?;
							xw.endtag("FontDict");
							xw.newline();
						}
						xw.endtag("FDArray");
						xw.newline();
					}
				}
				Conv::CharStrings => {
					if let Some(cs) = &self.char_strings {
						xw.begintag("CharStrings", &[]);
						xw.newline();
						char_strings_to_xml(xw, cs)?;
						xw.endtag("CharStrings");
						xw.newline();
					}
				}
				Conv::VarStore => {
					if let Some(store) = &self.var_store {
						let mut dummy = Font::new();
						store.to_xml(xw, &mut dummy, &[], "VarStore")?;
					}
				}
				conv => {
					if let Some(value) = self.get(name) {
						write_value(xw, conv, name, &value);
					}
				}
			}
		}
		ignored_keys(xw, &self.raw, &order);
		Ok(())
	}
}

impl CffFontSet {
	/// `CFFFontSet.toXML`. The charstrings are decompiled first, as
	/// `TopDict.toXML` does.
	pub fn to_xml(&self, xw: &mut XmlWriter) -> Result<()> {
		let mut set = self.clone();
		set.decompile_all_char_strings()?;
		xw.simpletag("major", &[("value", &set.major)]);
		xw.newline();
		xw.simpletag("minor", &[("value", &set.minor)]);
		xw.newline();
		for (name, top) in set.font_names.iter().zip(&set.top_dicts) {
			xw.begintag("CFFFont", &[("name", name)]);
			xw.newline();
			top.to_xml(xw)?;
			xw.endtag("CFFFont");
			xw.newline();
		}
		xw.newline();
		xw.begintag("GlobalSubrs", &[]);
		xw.newline();
		subrs_to_xml(xw, &set.global_subrs)?;
		xw.endtag("GlobalSubrs");
		xw.newline();
		Ok(())
	}
}
