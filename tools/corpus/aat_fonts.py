"""Wraps the binary test data of fontTools' AAT table tests into fonts.

For each `*_DATA` bytes constant of the test modules below, a minimal TrueType
font is written whose table is that data, stored as is. The glyph order is the
longest one used by the module's tests, padded so that any glyph ID resolves.

Usage: python aat_fonts.py <fonttools Tests dir> <output dir>
"""

import importlib.util
import inspect
import os
import sys

from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib.tables.DefaultTable import DefaultTable
from fontTools.misc.testTools import FakeFont

MODULES = {
	"_a_n_k_r_test": "ankr",
	"_b_s_l_n_test": "bsln",
	"_c_i_d_g_test": "cidg",
	"_g_c_i_d_test": "gcid",
	"_l_c_a_r_test": "lcar",
	"_m_o_r_t_test": "mort",
	"_m_o_r_x_test": "morx",
	"_o_p_b_d_test": "opbd",
	"_p_r_o_p_test": "prop",
}

NUM_GLYPHS = 1200


def load(path, name):
	spec = importlib.util.spec_from_file_location(name, path)
	module = importlib.util.module_from_spec(spec)
	spec.loader.exec_module(module)
	return module


def glyph_order(module):
	"""The longest glyph order among the module's test fonts."""
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
	# Some tests repeat names (an unformatted "X.alt%d"); keep them unique.
	seen = set()
	for i, g in enumerate(best):
		if g in seen:
			best[i] = "glyph%05d" % i
		seen.add(best[i])
	names = set(best)
	for i in range(len(best), NUM_GLYPHS):
		name = "glyph%05d" % i
		if name not in names:
			best.append(name)
	return best


def build(order, tag, data, path):
	fb = FontBuilder(1000, isTTF=True)
	fb.setupGlyphOrder(order)
	fb.setupCharacterMap({})
	empty = TTGlyphPen(None).glyph()
	fb.setupGlyf({g: empty for g in order})
	fb.setupHorizontalMetrics({g: (500, 0) for g in order})
	fb.setupHorizontalHeader()
	fb.setupNameTable({"familyName": "AAT Test", "styleName": "Regular"})
	fb.setupOS2()
	fb.setupPost(keepGlyphNames=True)
	# Keep the data as is: a raw DefaultTable.
	raw = DefaultTable(tag)
	raw.data = data
	fb.font[tag] = raw
	fb.save(path)


def main(tests, out):
	os.makedirs(out, exist_ok=True)
	tables = os.path.join(tests, "ttLib", "tables")
	sys.path.insert(0, tables)
	for name, tag in MODULES.items():
		module = load(os.path.join(tables, name + ".py"), name)
		order = glyph_order(module)
		for const, value in vars(module).items():
			if not const.endswith("_DATA") or not isinstance(value, bytes):
				continue
			path = os.path.join(out, f"{tag}-{const}.ttf")
			try:
				build(order, tag, value, path)
			except Exception as e:
				print(f"{path}: {type(e).__name__}: {e}")


main(sys.argv[1], sys.argv[2])
