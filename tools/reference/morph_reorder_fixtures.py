"""Generates the fixtures of `test_reorder_morph` in
crates/fontoxide-ttlib/tests/reorder_glyphs.rs.

fontTools' reorderGlyphs has no test with `mort` or `morx` tables. This script
takes the binary data of fontTools' `mort` and `morx` table tests (the `*_DATA`
constants of Tests/ttLib/tables/_m_o_r_t_test.py and _m_o_r_x_test.py), decompiles
each into a font whose glyph order is the longest one of the module's tests,
reverses the glyph order after `.notdef` with `reorderGlyphs`, and saves in
crates/fontoxide-ttlib/tests/data/reorder_glyphs/morph:

- <tag>.glyphs.txt: the glyph order of the module, one name per line.
- <tag>-<CONST>.bin: the table data.
- <tag>-<CONST>.expected.ttx: the table after `reorderGlyphs`, as TTX.
- <tag>-<CONST>.expected.bin: the table after `reorderGlyphs`, compiled.

    uv run --project tools/reference python tools/reference/morph_reorder_fixtures.py
"""

import importlib.util
import inspect
import io
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "upstream" / "fonttools" / "Lib"))
sys.dont_write_bytecode = True

from fontTools.misc.testTools import FakeFont
from fontTools.ttLib import TTFont, newTable
from fontTools.ttLib.reorderGlyphs import reorderGlyphs

TABLES = ROOT / "upstream" / "fonttools" / "Tests" / "ttLib" / "tables"
OUT = ROOT / "crates" / "fontoxide-ttlib" / "tests" / "data" / "reorder_glyphs" / "morph"
MODULES = {"_m_o_r_t_test": "mort", "_m_o_r_x_test": "morx"}


def load(name):
	spec = importlib.util.spec_from_file_location(name, TABLES / f"{name}.py")
	module = importlib.util.module_from_spec(spec)
	spec.loader.exec_module(module)
	return module


def glyph_order(module):
	"""The longest glyph order among the module's test fonts, with unique names
	(as tools/corpus/aat_fonts.py, without the padding)."""
	best = []
	for _, cls in inspect.getmembers(module, inspect.isclass):
		setup = cls.__dict__.get("setUpClass")
		if setup is None:
			continue
		try:
			setup.__get__(None, cls)()
		except Exception:
			continue
		font = getattr(cls, "font", None)
		if isinstance(font, FakeFont) and len(font.getGlyphOrder()) > len(best):
			best = list(font.getGlyphOrder())
	if not best or best[0] != ".notdef":
		best = [".notdef"] + [g for g in best if g != ".notdef"]
	seen = set()
	for i, g in enumerate(best):
		if g in seen:
			best[i] = "glyph%05d" % i
		seen.add(best[i])
	return best


def main():
	OUT.mkdir(parents=True, exist_ok=True)
	for name, tag in MODULES.items():
		module = load(name)
		order = glyph_order(module)
		(OUT / f"{tag}.glyphs.txt").write_text("\n".join(order) + "\n", encoding="utf-8")
		for const, data in vars(module).items():
			if not const.endswith("_DATA") or not isinstance(data, bytes):
				continue
			font = TTFont()
			font.setGlyphOrder(order)
			table = newTable(tag)
			table.decompile(data, font)
			font[tag] = table
			reorderGlyphs(font, order[:1] + list(reversed(order[1:])))
			stem = f"{tag}-{const}"
			(OUT / f"{stem}.bin").write_bytes(data)
			with open(OUT / f"{stem}.expected.ttx", "w", encoding="utf-8", newline="\n") as f:
				font.saveXML(f, tables=[tag], newlinestr="\n")
			(OUT / f"{stem}.expected.bin").write_bytes(font[tag].compile(font))
			print(stem)


if __name__ == "__main__":
	main()
