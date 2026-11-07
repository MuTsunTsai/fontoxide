"""Produce reference data from the pinned fontTools for fontoxide.

Run through the uv project in this directory, e.g.

	uv run --project tools/reference python tools/reference/reference.py info

Subcommands:

	info                     Print the fontTools version and optional features as JSON.
	batch JOBS RESULTS       Run a list of jobs (see `run_job`) and write per-job results.
	export-otdata OUT        Dump fontTools.ttLib.tables.otData as JSON.
	export-sstruct OUT       Dump every sstruct format string found in ttLib.tables.
	export-data CRATES_DIR   Write embedded data (AGL, standard glyph order, table info).
	list-tests OUT           List fontTools' test suite (via pytest collection) as JSON.

The batch interface exists so that `cargo xtask parity` can process thousands of
fonts with a single interpreter start-up.
"""

import argparse
import importlib
import json
import pkgutil
import re
import sys
import traceback
from pathlib import Path

# Import fontTools from the submodule rather than from an installed package, and
# never write bytecode caches into the submodule.
sys.dont_write_bytecode = True
UPSTREAM = Path(__file__).resolve().parents[2] / "upstream" / "fonttools"
sys.path.insert(0, str(UPSTREAM / "Lib"))

import fontTools  # noqa: E402
from fontTools.misc import sstruct
from fontTools.ttLib import TTFont
from fontTools.ttLib.tables import otBase


def info() -> dict:
	return {
		"fonttools_version": fontTools.version,
		"fonttools_path": str(Path(fontTools.__file__).parent),
		"python_version": sys.version.split()[0],
		"have_uharfbuzz": otBase.have_uharfbuzz,
	}


#####################################################################################################
# Batch jobs
#####################################################################################################

# Every job is a JSON object:
#
#   {"op": "ttx" | "roundtrip", "input": "<font path>", "output": "<path>",
#    "font_number": 0, "repacker": false}
#
# - ttx:        decompile every table and write the TTX dump (`ttx -o OUT FONT`).
# - roundtrip:  decompile every table, then compile the font again. This forces every
#               table through fontTools' compiler, unlike a plain load-and-save which
#               copies untouched tables verbatim.
#
# Both ops keep head.modified as-is (recalcTimestamp=False) so that the output is
# deterministic; fontoxide must be run with the equivalent options.


def open_font(job: dict) -> TTFont:
	font = TTFont(
		job["input"],
		fontNumber=job.get("font_number", 0),
		recalcTimestamp=False,
		lazy=False,
	)
	font.cfg[otBase.USE_HARFBUZZ_REPACKER] = bool(job.get("repacker", False))
	return font


def run_job(job: dict) -> None:
	output = Path(job["output"])
	output.parent.mkdir(parents=True, exist_ok=True)
	op = job["op"]
	if op in ("ttc-ttx", "ttc-roundtrip"):
		from fontTools.ttLib import TTCollection

		collection = TTCollection(job["input"], recalcTimestamp=False, lazy=False)
		for font in collection.fonts:
			font.cfg[otBase.USE_HARFBUZZ_REPACKER] = bool(job.get("repacker", False))
		if op == "ttc-ttx":
			collection.saveXML(output)
		else:
			for font in collection.fonts:
				font.ensureDecompiled()
			collection.save(output)
		return
	if op == "fromttx":
		# The input is a TTX file (fontTools' own dump of the font).
		font = TTFont(recalcTimestamp=False)
		font.cfg[otBase.USE_HARFBUZZ_REPACKER] = bool(job.get("repacker", False))
		font.importXML(job["input"])
		font.save(output, reorderTables=True)
		return
	font = open_font(job)
	if op == "ttx":
		font.saveXML(output)
	elif op == "roundtrip":
		font.ensureDecompiled()
		font.save(output, reorderTables=True)
	else:
		raise ValueError(f"unknown op: {op}")


def batch(jobs_path: Path, results_path: Path) -> None:
	jobs = json.loads(jobs_path.read_text(encoding="utf-8"))
	results = []
	for job in jobs:
		try:
			run_job(job)
			results.append({"ok": True})
		except Exception as e:
			results.append({
				"ok": False,
				"error": f"{type(e).__name__}: {e}",
				"traceback": traceback.format_exc(),
			})
	results_path.write_text(json.dumps(results, indent="\t"), encoding="utf-8", newline="\n")


#####################################################################################################
# Code generation data
#####################################################################################################


def export_otdata() -> dict:
	from fontTools.ttLib.tables.otData import otData

	tables = []
	for name, fields in otData:
		tables.append({
			"name": name,
			"fields": [
				{
					"type": f.type,
					"name": f.name,
					"repeat": f.repeat,
					"aux": f.aux,
					"condition": f.condition,
					"description": f.description,
				}
				for f in fields
			],
		})
	return {"fonttools_version": fontTools.version, "tables": tables}


def export_sstruct() -> dict:
	"""Collect module-level sstruct format strings from fontTools.ttLib.tables.*.

	Table modules declare their fixed-size layouts as module-level strings, e.g.
	`headFormat` in `_h_e_a_d.py`. Anything that `sstruct.getformat` accepts and that
	declares at least one named field is exported.
	"""
	import fontTools.ttLib.tables as tables_pkg

	formats = []
	for module_info in sorted(pkgutil.iter_modules(tables_pkg.__path__), key=lambda m: m.name):
		module_name = f"{tables_pkg.__name__}.{module_info.name}"
		try:
			module = importlib.import_module(module_name)
		except ImportError as e:
			print(f"skipping {module_name}: {e}", file=sys.stderr)
			continue
		for attr, value in sorted(vars(module).items()):
			if not isinstance(value, str) or "format" not in attr.lower():
				continue
			try:
				struct_format, names, fixes = sstruct.getformat(value)
			except Exception:
				continue
			if not names:
				continue
			formats.append({
				"module": module_name,
				"name": attr,
				"source": value,
				"struct_format": struct_format,
				"fields": list(names),
				# Fixed-point fields: name -> number of fraction bits.
				"fixes": fixes,
			})
	return {"fonttools_version": fontTools.version, "formats": formats}


def export_ottables() -> dict:
	"""The otTables classes and their converters, as fontTools builds them from otData.

	This is what fontTools actually uses to read, write and dump OpenType Layout and
	related tables, so fontoxide's code generator follows it instead of interpreting
	otData itself.
	"""
	import inspect
	from fontTools.ttLib.tables import otBase, otConverters, otTables
	from fontTools.ttLib.tables.otData import otData

	hooks = [
		"postRead", "preWrite", "toXML", "toXML2", "fromXML", "populateDefaults",
		"decompile", "compile", "readFormat", "writeFormat",
	]

	def class_name(cls):
		return cls.__name__ if cls is not None else None

	def aux_value(aux):
		if aux is None or isinstance(aux, int):
			return aux
		if isinstance(aux, str):
			return aux
		# compiled condition expressions keep their source in co_consts? no: use the spec.
		return None

	spec_aux = {}
	for name, fields in otData:
		for f in fields:
			spec_aux[(name, f.name)] = (f.aux, f.condition)

	def converter(owner, conv):
		aux, condition = spec_aux.get((owner, conv.name), (conv.aux, None))
		d = {
			"converter": type(conv).__name__,
			"name": conv.name,
			"repeat": conv.repeat,
			"aux": aux if isinstance(aux, (int, str)) or aux is None else None,
			"condition": condition,
			"tableClass": class_name(getattr(conv, "tableClass", None)),
			"staticSize": getattr(conv, "staticSize", None),
			"isCount": conv.isCount,
			"isPropagated": conv.isPropagated,
			"isLookupType": conv.isLookupType,
			"description": conv.description,
		}
		value_conv = getattr(conv, "converter", None)
		if isinstance(value_conv, otConverters.BaseConverter):
			d["valueConverter"] = type(value_conv).__name__
			d["valueTableClass"] = class_name(getattr(value_conv, "tableClass", None))
		for attr in ("writeNullOffset", "DEFAULT", "enumClass", "itemClass", "itemConverterClass"):
			if hasattr(conv, attr):
				v = getattr(conv, attr)
				if inspect.isclass(v):
					v = v.__name__
				elif not isinstance(v, (int, float, str, bool, type(None))):
					v = True  # a method or other callable: only presence matters
				d[attr] = v
		return d

	classes = []
	seen = set()
	for name, _ in otData:
		m = re.match(r"(.*)Format(\d+)$", name)
		base_name = m.group(1) if m and hasattr(otTables, m.group(1)) else name
		if base_name in seen:
			continue
		seen.add(base_name)
		cls = getattr(otTables, base_name)
		if issubclass(cls, otBase.UInt8FormatSwitchingBaseTable):
			kind = "FormatSwitching8"
		elif issubclass(cls, otBase.FormatSwitchingBaseTable):
			kind = "FormatSwitching16"
		else:
			kind = "BaseTable"
		entry = {
			"name": base_name,
			"kind": kind,
			"bases": [b.__name__ for b in cls.__mro__[1:] if b.__module__.startswith("fontTools")],
			"hooks": [h for h in hooks if h in vars(cls)],
			"inheritedHooks": [
				h for h in hooks
				if any(h in vars(b) for b in cls.__mro__[1:] if b not in (otBase.BaseTable, otBase.FormatSwitchingBaseTable, otBase.UInt8FormatSwitchingBaseTable, object))
			],
			"attrs": {a: getattr(cls, a) for a in ("LookupType", "sortCoverageLast", "DontShare") if hasattr(cls, a)},
		}
		if kind == "BaseTable":
			entry["converters"] = [converter(base_name, c) for c in cls.converters]
		else:
			entry["formats"] = {
				str(fmt): [converter(f"{base_name}Format{fmt}", c) for c in convs]
				for fmt, convs in sorted(cls.converters.items())
			}
		classes.append(entry)

	lookup_types = {
		tag: {str(t): c.__name__ for t, c in sorted(types.items())}
		for tag, types in otTables.lookupTypes.items()
	}
	feature_params = {tag: c.__name__ for tag, c in sorted(otTables.featureParamTypes.items())}
	return {
		"fonttools_version": fontTools.version,
		"classes": classes,
		"lookupTypes": lookup_types,
		"featureParamTypes": feature_params,
	}


def export_tables() -> dict:
	"""Per-tag facts about the table classes in fontTools.ttLib.tables.

	For every module that defines a table class: the tag, the class name, and the
	class-level `dependencies` (which may be inherited from a base class).
	"""
	import fontTools.ttLib.tables as tables_pkg
	from fontTools.ttLib import getTableClass, identifierToTag

	tables = []
	for module_info in sorted(pkgutil.iter_modules(tables_pkg.__path__), key=lambda m: m.name):
		name = module_info.name
		try:
			tag = identifierToTag(name)
		except (AssertionError, ValueError, IndexError):
			continue
		if len(tag) != 4:
			continue
		try:
			cls = getTableClass(tag)
		except (ImportError, AttributeError):
			continue
		if cls.__name__ != "table_" + name:
			continue
		tables.append({
			"tag": tag,
			"module": f"{tables_pkg.__name__}.{name}",
			"class": cls.__name__,
			"dependencies": list(cls.dependencies),
		})
	tables.sort(key=lambda t: t["tag"])
	return {"fonttools_version": fontTools.version, "tables": tables}


def export_unicode_names(out_dir: Path) -> None:
	"""Character names from the unicodedata that fontTools.unicode uses.

	Names of the form PREFIX-XXXX (CJK ideographs and similar) and Hangul syllables
	are derived algorithmically; they are written as ranges and checked here. All
	other names are listed.
	"""
	import re

	try:
		import unicodedata2 as unicodedata
	except ImportError:
		import unicodedata

	out_dir.mkdir(parents=True, exist_ok=True)
	explicit = []
	ranges = []  # [prefix, start, end]
	for cp in range(0x110000):
		name = unicodedata.name(chr(cp), None)
		if name is None:
			continue
		m = re.fullmatch(r"(.*-)([0-9A-F]{4,6})", name)
		if m and int(m.group(2), 16) == cp:
			prefix = m.group(1)
		elif name.startswith("HANGUL SYLLABLE ") and 0xAC00 <= cp <= 0xD7A3:
			prefix = "HANGUL SYLLABLE "
		else:
			explicit.append(f"{cp:04X};{name}")
			continue
		if ranges and ranges[-1][0] == prefix and ranges[-1][2] == cp - 1:
			ranges[-1][2] = cp
		else:
			ranges.append([prefix, cp, cp])
	# The Rust side computes Hangul syllable names; make sure the range is whole.
	assert ["HANGUL SYLLABLE ", 0xAC00, 0xD7A3] in ranges
	with open(out_dir / "names.txt", "w", encoding="utf-8", newline="\n") as f:
		f.write(f"# Unicode {unicodedata.unidata_version} character names, from fontTools' unicodedata\n")
		f.write("\n".join(explicit) + "\n")
	with open(out_dir / "name_ranges.txt", "w", encoding="utf-8", newline="\n") as f:
		f.write(f"# Unicode {unicodedata.unidata_version} algorithmic names: prefix;first;last\n")
		for prefix, start, end in ranges:
			f.write(f"{prefix};{start:04X};{end:04X}\n")


# The Python codecs that fontTools.misc.encodingTools can return (besides UTF-16BE,
# ASCII and Latin-1, which fontoxide implements directly).
SINGLE_BYTE_CODECS = [
	"mac_roman", "mac_iceland", "mac_turkish", "mac_croatian", "mac_latin2",
	"mac_romanian", "mac_greek", "mac_cyrillic",
]
MULTI_BYTE_CODECS = ["shift_jis", "gb2312", "big5", "euc_kr", "johab"]


def export_codecs(out_dir: Path) -> None:
	"""Decoding and encoding tables of Python codecs used for name records.

	Single-byte codecs: one line with 256 code points (hex, "-" if undefined).
	Multi-byte codecs: the bytes that decode alone ("single"), then every two-byte
	sequence starting with a byte that does not ("double"), then the encoding of every
	BMP character whose encoding differs from what the decoding tables imply.
	"""
	out_dir.mkdir(parents=True, exist_ok=True)
	for name in SINGLE_BYTE_CODECS:
		cps = []
		for b in range(256):
			try:
				s = bytes([b]).decode(name)
				assert len(s) == 1
				cps.append(f"{ord(s):04X}")
			except UnicodeDecodeError:
				cps.append("-")
		with open(out_dir / f"{name}.txt", "w", encoding="utf-8", newline="\n") as f:
			f.write(f"# Python {sys.version.split()[0]} codec {name}: byte -> code point\n")
			f.write(" ".join(cps) + "\n")
	for name in MULTI_BYTE_CODECS:
		single = {}
		for b in range(256):
			try:
				s = bytes([b]).decode(name)
				assert len(s) == 1
				single[b] = ord(s)
			except UnicodeDecodeError:
				pass
		double = {}
		for lead in range(256):
			if lead in single:
				continue
			for trail in range(256):
				try:
					s = bytes([lead, trail]).decode(name)
				except UnicodeDecodeError:
					continue
				if len(s) == 1:
					double[(lead << 8) | trail] = ord(s)
		decoded = {}
		for b, cp in single.items():
			decoded.setdefault(cp, bytes([b]))
		for seq, cp in sorted(double.items()):
			decoded.setdefault(cp, bytes([seq >> 8, seq & 0xFF]))
		encode_diff = []
		for cp in range(0x10000):
			if 0xD800 <= cp <= 0xDFFF:
				continue
			try:
				enc = chr(cp).encode(name)
			except UnicodeEncodeError:
				enc = None
			if enc != decoded.get(cp):
				encode_diff.append(f"{cp:04X}:{enc.hex().upper() if enc is not None else '-'}")
		with open(out_dir / f"{name}.txt", "w", encoding="utf-8", newline="\n") as f:
			f.write(f"# Python {sys.version.split()[0]} codec {name}\n")
			f.write("single " + " ".join(f"{b:02X}:{cp:04X}" for b, cp in sorted(single.items())) + "\n")
			f.write("double " + " ".join(f"{s:04X}:{cp:04X}" for s, cp in sorted(double.items())) + "\n")
			f.write("encode " + " ".join(encode_diff) + "\n")


def export_data(out_dir: Path) -> None:
	"""Write data tables that fontoxide embeds verbatim."""
	from fontTools import agl
	from fontTools.ttLib.standardGlyphOrder import standardGlyphOrder

	agl_dir = out_dir / "fontoxide-agl" / "data"
	agl_dir.mkdir(parents=True, exist_ok=True)
	(agl_dir / "glyphlist.txt").write_text(agl._aglText, encoding="utf-8", newline="\n")
	(agl_dir / "aglfn.txt").write_text(agl._aglfnText, encoding="utf-8", newline="\n")
	(agl_dir / "zapfdingbats.txt").write_text(agl._AGL_ZAPF_DINGBATS, encoding="utf-8", newline="\n")

	export_unicode_names(out_dir / "fontoxide-unicodedata" / "data")
	export_codecs(out_dir / "fontoxide-encodings" / "data" / "codecs")

	data_dir = out_dir / "fontoxide-ttlib" / "codegen"
	write_json(data_dir / "tables.json", export_tables())
	write_json(data_dir / "ottables.json", export_ottables())
	write_json(
		data_dir / "standardGlyphOrder.json",
		{"fonttools_version": fontTools.version, "names": standardGlyphOrder},
	)

	from fontTools import cffLib

	write_json(
		data_dir / "cffStrings.json",
		{
			"fonttools_version": fontTools.version,
			"standardStrings": cffLib.cffStandardStrings,
			"ISOAdobe": cffLib.cffISOAdobeStrings,
			"Expert": cffLib.cffIExpertStrings,
			"ExpertSubset": cffLib.cffExpertSubsetStrings,
		},
	)


#####################################################################################################
# Upstream test inventory
#####################################################################################################


# Modules whose doctests cannot be collected because they import rendering or plotting
# backends that are platform-specific (Quartz) or need native libraries (cairo,
# FreeType). Their test files under Tests/ are still collected. Anything else that
# fails to import is reported as an error.
UNTRACKED_DOCTEST_MODULES = {
	"Lib/fontTools/misc/symfont.py": "needs sympy; code-generation helper",
	"Lib/fontTools/pens/freetypePen.py": "needs freetype-py",
	"Lib/fontTools/pens/quartzPen.py": "needs macOS Quartz",
	"Lib/fontTools/pens/reportLabPen.py": "needs reportlab",
	"Lib/fontTools/varLib/interpolatablePlot.py": "needs pycairo",
	"Lib/fontTools/varLib/plot.py": "needs matplotlib",
}


def list_tests() -> dict:
	"""Collect fontTools' test suite with pytest, using upstream's own configuration.

	This covers Tests/ as well as the doctests in Lib/fontTools (upstream runs with
	--doctest-modules), and sees tests that are generated at import time. Nothing is
	executed. Parametrized cases are folded into their test function, so every entry
	is a pytest node id without the "[...]" suffix, plus the number of cases.

	Modules that fail to import (usually a missing optional dependency) are reported
	under "errors" so that they are not mistaken for having no tests.
	"""
	import os

	import pytest

	class Collector:
		def __init__(self):
			self.items = []
			self.errors = []

		def pytest_collection_modifyitems(self, items):
			self.items = list(items)

		def pytest_collectreport(self, report):
			# A skipped module (pytest.importorskip) hides its tests just like a failed one.
			if (report.failed or report.skipped) and report.nodeid not in UNTRACKED_DOCTEST_MODULES:
				self.errors.append({"id": report.nodeid, "error": str(report.longrepr).splitlines()[-1]})

	collector = Collector()
	os.chdir(UPSTREAM)
	code = pytest.main(
		["--collect-only", "-q", "-p", "no:cacheprovider", "--continue-on-collection-errors"],
		plugins=[collector],
	)
	if code not in (pytest.ExitCode.OK, pytest.ExitCode.INTERRUPTED, pytest.ExitCode.TESTS_FAILED):
		print(f"pytest exited with {code!r}", file=sys.stderr)

	cases: dict[str, int] = {}
	for item in collector.items:
		test_id = item.nodeid.split("[", 1)[0]
		cases[test_id] = cases.get(test_id, 0) + 1
	return {
		"fonttools_version": fontTools.version,
		"tests": [{"id": test_id, "cases": n} for test_id, n in sorted(cases.items())],
		"errors": collector.errors,
	}


#####################################################################################################


def write_json(path: Path, data) -> None:
	path.parent.mkdir(parents=True, exist_ok=True)
	path.write_text(json.dumps(data, indent="\t", ensure_ascii=False) + "\n", encoding="utf-8", newline="\n")


def main() -> None:
	parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
	sub = parser.add_subparsers(dest="command", required=True)
	sub.add_parser("info")
	p = sub.add_parser("batch")
	p.add_argument("jobs", type=Path)
	p.add_argument("results", type=Path)
	p = sub.add_parser("export-otdata")
	p.add_argument("out", type=Path)
	p = sub.add_parser("export-sstruct")
	p.add_argument("out", type=Path)
	p = sub.add_parser("export-data")
	p.add_argument("out", type=Path)
	p = sub.add_parser("list-tests")
	p.add_argument("out", type=Path)
	args = parser.parse_args()

	if args.command == "info":
		print(json.dumps(info()))
	elif args.command == "batch":
		batch(args.jobs, args.results)
	elif args.command == "export-otdata":
		write_json(args.out, export_otdata())
	elif args.command == "export-sstruct":
		write_json(args.out, export_sstruct())
	elif args.command == "export-data":
		export_data(args.out)
	elif args.command == "list-tests":
		write_json(args.out, list_tests())


if __name__ == "__main__":
	main()
