"""Generates `crates/fontoxide-varlib/src/merger/walk_generated.rs`.

fontTools' `varLib.merger` walks OpenType Layout tables generically: for each
object it merges the attributes in sorted order (`sorted(vars(obj))`), lists
item by item, and compares everything else for equality. The Rust tables are
static types, so this script writes that walk for every type generated from
otData (`crates/fontoxide-ttlib/src/generated/ot.rs`): the Python attribute
names come from the doc comments of the fields.

`cargo xtask codegen` runs it after regenerating ot.rs; to run it alone, from the
repository root: `uv run --project tools/reference python tools/reference/varlib_merger_codegen.py`.
It needs no fontTools; it only parses the generated Rust source.
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / "crates/fontoxide-ttlib/src/generated/ot.rs"
TARGET = ROOT / "crates/fontoxide-varlib/src/merger/walk_generated.rs"

# Types written by hand in ttlib that the walk treats as objects.
CUSTOM_WALK = {
	"Coverage",
	"ClassDef",
	"SingleSubst",
	"MultipleSubst",
	"AlternateSubst",
	"LigatureSubst",
	"ClipList",
	"DeltaSetIndexMap",
	"VarIdxMap",
	"ValueRecord",
}

# Types with a merger of their own (`Merger.merger(clazz)`): their
# `merge_dispatch` calls `special::<snake name>`.
SPECIAL = {
	"Anchor",
	"SinglePos",
	"PairPos",
	"PairSet",
	"MarkBasePos",
	"MarkMarkPos",
	"CursivePos",
	"EntryExitRecord",
	"Lookup",
	"CaretValue",
	"BaseCoord",
	"BaseGlyphList",
	"LayerList",
	"Paint",
	"ClipBox",
	"Affine2x3",
	"ColorStop",
	"ColorLine",
}

# Attributes with a merger of their own (`Merger.merger(clazz, attrs)`).
SPECIAL_ATTRS = {
	("GDEF", "GlyphClassDef"): "gdef_glyph_class_def",
	("GDEF", "GlyphClassDef2"): "gdef_glyph_class_def",
}

# Fields that are not Python attributes.
SKIP_FIELDS = {"dont_share"}

INT_REPR = {"u8", "u16", "u32", "i16", "i32", "i64", "u64"}


def snake(name):
	s = re.sub(r"(?<=[a-z0-9])([A-Z])", r"_\1", name)
	s = re.sub(r"(?<=[A-Z])([A-Z][a-z])", r"_\1", s)
	return s.lower()


def parse(text):
	structs = {}  # name -> (doc, fields[(pyname, rustname, type)])
	enums = {}  # name -> (doc, variants[(variant, inner or None, label)])
	lines = text.splitlines()
	i = 0
	doc = None
	while i < len(lines):
		line = lines[i]
		m = re.match(r"^/// (.*)$", line)
		if m:
			doc = m.group(1)
		m = re.match(r"^pub struct (\w+) \{\}?$", line)
		if m:
			name = m.group(1)
			fields = []
			if not line.endswith("{}"):
				i += 1
				pydoc = None
				while lines[i] != "}":
					fm = re.match(r"^\t/// `(\w+)` \((\w+)\)\.$", lines[i])
					if fm:
						pydoc = (fm.group(1), fm.group(2))
					fm = re.match(r"^\tpub (\w+): (.*),$", lines[i])
					if fm:
						if fm.group(1) not in SKIP_FIELDS:
							if pydoc is None:
								sys.exit(f"no Python name for {name}.{fm.group(1)}")
							fields.append((pydoc[0], fm.group(1), fm.group(2), pydoc[1]))
						pydoc = None
					i += 1
			structs[name] = (doc, fields)
		m = re.match(r"^pub enum (\w+) \{$", line)
		if m:
			name = m.group(1)
			variants = []
			i += 1
			label = None
			while lines[i] != "}":
				vm = re.match(r"^\t/// (.*)$", lines[i])
				if vm:
					label = vm.group(1)
				vm = re.match(r"^\t(\w+)\((\w+)\),$", lines[i])
				if vm:
					variants.append((vm.group(1), vm.group(2), label))
				i += 1
			enums[name] = (doc, variants)
		i += 1
	return structs, enums


def class_of(doc, name):
	m = re.match(r"^`(\w+)`", doc or "")
	return m.group(1) if m else name


def main():
	text = SOURCE.read_text(encoding="utf-8")
	structs, enums = parse(text)
	walk_types = set(structs) | set(enums) | CUSTOM_WALK
	# Format variants: struct name -> python class.
	format_variant = {}
	for ename, (edoc, variants) in enums.items():
		for vname, inner, label in variants:
			if vname.startswith("Format") and inner != "u16":
				format_variant[inner] = class_of(edoc, ename)

	def field_code(pyname, rname, ty, conv, owner):
		get = f"lst.iter().map(|l| &l.{rname}).collect::<Vec<_>>()"
		m = re.match(r"^Option<Box<(\w+)>>$", ty)
		if m and m.group(1) in walk_types:
			return f"merge_opt(m, self.{rname}.as_deref_mut(), &lst.iter().map(|l| l.{rname}.as_deref()).collect::<Vec<_>>())"
		m = re.match(r"^Option<(\w+)>$", ty)
		if m and m.group(1) in walk_types:
			return f"merge_opt(m, self.{rname}.as_mut(), &lst.iter().map(|l| l.{rname}.as_ref()).collect::<Vec<_>>())"
		m = re.match(r"^Vec<Option<Box<(\w+)>>>$", ty)
		if m and m.group(1) in walk_types:
			return f"merge_opt_list(m, &mut self.{rname}, &{get})"
		m = re.match(r"^Vec<Option<(\w+)>>$", ty)
		if m and m.group(1) in walk_types:
			return f"merge_opt_plain_list(m, &mut self.{rname}, &{get})"
		m = re.match(r"^Vec<(\w+)>$", ty)
		if m and m.group(1) in walk_types:
			return f"merge_list(m, &mut self.{rname}, &{get})"
		if ty in walk_types:
			return f"merge_struct(m, &mut self.{rname}, &{get})"
		m = re.match(r"^Vec<(\w+)>$", ty)
		if m:
			inner = m.group(1)
			return f"merge_eq_list(m, &self.{rname}, &{get}, {repr_fn(inner, conv)})"
		return f"merge_eq(m, &self.{rname}, &{get}, {repr_fn(ty, conv)})"

	def repr_fn(ty, conv):
		if ty in INT_REPR:
			return "repr_int"
		if ty == "f64":
			return "repr_float"
		if ty == "bool":
			return "repr_bool"
		if ty == "Tag":
			return "repr_tag"
		if ty == "String":
			return "repr_str"
		if ty == "Option<u32>":
			return "repr_opt_int"
		return "repr_debug"

	out = []
	out.append("// @generated by `tools/reference/varlib_merger_codegen.py` from")
	out.append("// `crates/fontoxide-ttlib/src/generated/ot.rs`. Do not edit.")
	out.append("")
	out.append("//! The generic walk of `fontTools.varLib.merger` over the otData types.")
	out.append("")
	out.append("#![allow(clippy::all, unused_variables)]")
	out.append("")
	out.append("use fontoxide_ttlib::ot::types as ot;")
	out.append("")
	out.append("use super::special;")
	out.append("use super::walk::*;")
	out.append("use super::{MResult, Merger};")
	out.append("")
	for name in sorted(structs):
		doc, fields = structs[name]
		pyclass = format_variant.get(name) or class_of(doc, name)
		keys = [f[0] for f in fields]
		if name in format_variant:
			keys.append("Format")
		keys = sorted(set(keys))
		out.append(f"impl Walk for ot::{name} {{")
		out.append("\tfn py_type(&self) -> &'static str {")
		out.append(f"\t\t{pyclass!r}".replace("'", '"'))
		out.append("\t}")
		out.append("\tfn py_keys(&self) -> &'static [&'static str] {")
		out.append("\t\t&[" + ", ".join(f'"{k}"' for k in keys) + "]")
		out.append("\t}")
		out.append("\tfn attr_names(&self) -> &'static [&'static str] {")
		out.append("\t\t&[" + ", ".join(f'"{f[0]}"' for f in fields) + "]")
		out.append("\t}")
		out.append(
			"\tfn merge_attr(&mut self, m: &mut Merger<'_>, lst: &[&Self], attr: &str) -> MResult {"
		)
		out.append("\t\tmatch attr {")
		for pyname, rname, ty, conv in fields:
			sp = SPECIAL_ATTRS.get((pyclass, pyname))
			if sp:
				code = f"special::{sp}(m, &mut self.{rname}, &lst.iter().map(|l| &l.{rname}).collect::<Vec<_>>())"
			else:
				code = field_code(pyname, rname, ty, conv, name)
			out.append(f'\t\t\t"{pyname}" => {code},')
		out.append("\t\t\t_ => Ok(()),")
		out.append("\t\t}")
		out.append("\t}")
		out.append(
			"\tfn merge_object(&mut self, m: &mut Merger<'_>, lst: &[&Self], exclude: &[&str]) -> MResult {"
		)
		for key in keys:
			if key == "Format":
				continue
			out.append(
				f'\t\tif !exclude.contains(&"{key}") {{ with_attr(m, "{key}", |m| self.merge_attr(m, lst, "{key}"))?; }}'
			)
		out.append("\t\tOk(())")
		out.append("\t}")
		if pyclass in SPECIAL and name not in format_variant:
			out.append(
				"\tfn merge_dispatch(&mut self, m: &mut Merger<'_>, lst: &[&Self]) -> MResult {"
			)
			out.append(f"\t\tspecial::{snake(pyclass)}(self, m, lst)")
			out.append("\t}")
		out.append("}")
		out.append("")
	for name in sorted(enums):
		doc, variants = enums[name]
		pyclass = class_of(doc, name)
		is_format = any(v[0].startswith("Format") for v in variants)
		out.append(f"impl Walk for ot::{name} {{")
		out.append("\tfn py_type(&self) -> &'static str {")
		if is_format:
			out.append(f'\t\t"{pyclass}"')
		else:
			out.append("\t\tmatch self {")
			for vname, inner, label in variants:
				out.append(f"\t\t\tSelf::{vname}(t) => t.py_type(),")
			out.append("\t\t}")
		out.append("\t}")
		out.append("\tfn py_keys(&self) -> &'static [&'static str] {")
		out.append("\t\tmatch self {")
		for vname, inner, label in variants:
			if inner == "u16":
				out.append(f'\t\t\tSelf::{vname}(_) => &["Format"],')
			else:
				out.append(f"\t\t\tSelf::{vname}(t) => t.py_keys(),")
		out.append("\t\t}")
		out.append("\t}")
		out.append("\tfn attr_names(&self) -> &'static [&'static str] {")
		out.append("\t\tmatch self {")
		for vname, inner, label in variants:
			if inner == "u16":
				out.append(f"\t\t\tSelf::{vname}(_) => &[],")
			else:
				out.append(f"\t\t\tSelf::{vname}(t) => t.attr_names(),")
		out.append("\t\t}")
		out.append("\t}")
		for method, extra_param, extra_arg in (
			("merge_attr", "attr: &str", "attr"),
			("merge_object", "exclude: &[&str]", "exclude"),
		):
			out.append(
				f"\tfn {method}(&mut self, m: &mut Merger<'_>, lst: &[&Self], {extra_param}) -> MResult {{"
			)
			if method == "merge_object" and is_format:
				out.append("\t\tcheck_same_format(m, self, lst)?;")
			out.append("\t\tmatch self {")
			for vname, inner, label in variants:
				if inner == "u16":
					out.append(f"\t\t\tSelf::{vname}(_) => Ok(()),")
					continue
				out.append(f"\t\t\tSelf::{vname}(t) => {{")
				out.append(
					f"\t\t\t\tlet l: Vec<&ot::{inner}> = lst.iter().filter_map(|x| if let Self::{vname}(v) = x {{ Some(v) }} else {{ None }}).collect();"
				)
				out.append(f"\t\t\t\tt.{method}(m, &l, {extra_arg})")
				out.append("\t\t\t}")
			out.append("\t\t}")
			out.append("\t}")
		if is_format:
			out.append("\tfn format_number(&self) -> Option<u16> {")
			out.append("\t\tmatch self {")
			for vname, inner, label in variants:
				if inner == "u16":
					out.append(f"\t\t\tSelf::{vname}(f) => Some(*f),")
				else:
					out.append(f"\t\t\tSelf::{vname}(_) => Some({vname[len('Format'):]}),")
			out.append("\t\t}")
			out.append("\t}")
		if pyclass in SPECIAL:
			out.append(
				"\tfn merge_dispatch(&mut self, m: &mut Merger<'_>, lst: &[&Self]) -> MResult {"
			)
			out.append(f"\t\tspecial::{snake(pyclass)}(self, m, lst)")
			out.append("\t}")
		else:
			if not is_format:
				out.append(
					"\tfn merge_dispatch(&mut self, m: &mut Merger<'_>, lst: &[&Self]) -> MResult {"
				)
				out.append("\t\tmatch self {")
				for vname, inner, label in variants:
					out.append(f"\t\t\tSelf::{vname}(t) => {{")
					out.append(
						f"\t\t\t\tlet l: Vec<&ot::{inner}> = lst.iter().filter_map(|x| if let Self::{vname}(v) = x {{ Some(v) }} else {{ None }}).collect();"
					)
					out.append("\t\t\t\tt.merge_dispatch(m, &l)")
					out.append("\t\t\t}")
				out.append("\t\t}")
				out.append("\t}")
		out.append("}")
		out.append("")
	TARGET.write_bytes(("\n".join(out) + "\n").encode("utf-8"))
	print(f"wrote {TARGET}")


if __name__ == "__main__":
	main()
