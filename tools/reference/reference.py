"""Produce reference data from the pinned fontTools for fontoxide.

Run through the uv project in this directory, e.g.

	uv run --project tools/reference python tools/reference/reference.py info

Subcommands:

	info                     Print the fontTools version and optional features as JSON.
	batch JOBS RESULTS       Run a list of jobs (see `run_job`) and write per-job results.
	export-otdata OUT        Dump fontTools.ttLib.tables.otData as JSON.
	export-sstruct OUT       Dump every sstruct format string found in ttLib.tables.
	list-tests OUT           List fontTools' test suite (via pytest collection) as JSON.

The batch interface exists so that `cargo xtask parity` can process thousands of
fonts with a single interpreter start-up.
"""

import argparse
import importlib
import json
import pkgutil
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
				"fields": names,
				# Fixed-point fields: name -> number of fraction bits.
				"fixes": fixes,
			})
	return {"fonttools_version": fontTools.version, "formats": formats}


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
	elif args.command == "list-tests":
		write_json(args.out, list_tests())


if __name__ == "__main__":
	main()
