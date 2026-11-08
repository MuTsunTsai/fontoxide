"""Writes TTX files whose GSUB or GPOS overflow 16-bit offsets when compiled.

Compiling them makes fontTools resolve the overflows (promoting lookups to
Extension lookups, unsharing and splitting subtables), which the parity runner
compares as `fromttx` cases (`*.parity.ttx`). Each case is checked to overflow
with the pure-Python serializer.

Usage: python overflow_ttx.py <output dir>
"""

import logging
import os
import sys

from fontTools.fontBuilder import FontBuilder
from fontTools.otlLib import builder as otl
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib import newTable
from fontTools.ttLib.tables import otTables as ot

REPACKER = "fontTools.ttLib.tables.otBase:USE_HARFBUZZ_REPACKER"


def glyph_names(n):
	return [".notdef"] + ["g%05d" % i for i in range(1, n)]


def base_font(glyphs):
	fb = FontBuilder(1000, isTTF=True)
	fb.setupGlyphOrder(glyphs)
	fb.setupCharacterMap({})
	empty = TTGlyphPen(None).glyph()
	fb.setupGlyf({g: empty for g in glyphs})
	fb.setupHorizontalMetrics({g: (500, 0) for g in glyphs})
	fb.setupHorizontalHeader()
	fb.setupNameTable({"familyName": "Overflow Test", "styleName": "Regular"})
	fb.setupOS2()
	fb.setupPost(keepGlyphNames=True)
	fb.font.cfg[REPACKER] = False
	return fb.font


def layout(tag, lookups, feature="test"):
	table = getattr(ot, tag)()
	table.Version = 0x00010000
	feat = ot.Feature()
	feat.FeatureParams = None
	feat.LookupListIndex = list(range(len(lookups)))
	feat.LookupCount = len(lookups)
	rec = ot.FeatureRecord()
	rec.FeatureTag = feature
	rec.Feature = feat
	lang = ot.DefaultLangSys()
	lang.LookupOrder = None
	lang.ReqFeatureIndex = 0xFFFF
	lang.FeatureIndex = [0]
	script = ot.Script()
	script.DefaultLangSys = lang
	script.LangSysRecord = []
	srec = ot.ScriptRecord()
	srec.ScriptTag = "DFLT"
	srec.Script = script
	table.ScriptList = ot.ScriptList()
	table.ScriptList.ScriptRecord = [srec]
	table.FeatureList = ot.FeatureList()
	table.FeatureList.FeatureRecord = [rec]
	table.LookupList = ot.LookupList()
	table.LookupList.Lookup = lookups
	wrapper = newTable(tag)
	wrapper.table = table
	return wrapper


def value(x):
	return otl.buildValue({"XAdvance": x})


def pair_pos_glyphs(glyphs):
	pairs = {}
	for i, first in enumerate(glyphs[1:601]):
		for j, second in enumerate(glyphs[1:31]):
			pairs[(first, second)] = (value(-(i * 31 + j) % 997 - 1), None)
	return "GPOS", [otl.buildLookup(otl.buildPairPosGlyphs(pairs, font_map(glyphs)))]


def pair_pos_classes(glyphs):
	firsts = [frozenset([g]) for g in glyphs[1:501]]
	seconds = [frozenset([g]) for g in glyphs[501:581]]
	pairs = {}
	for i, a in enumerate(firsts):
		for j, b in enumerate(seconds):
			pairs[(a, b)] = (value((i * 7 + j * 13) % 500 - 250), None)
	sub = otl.buildPairPosClassesSubtable(pairs, font_map(glyphs))
	return "GPOS", [otl.buildLookup([sub])]


def single_pos(glyphs):
	mapping = {g: value(-(i % 100 + 1)) for i, g in enumerate(glyphs[1:])}
	sub = otl.buildSinglePosSubtable(mapping, font_map(glyphs))
	return "GPOS", [otl.buildLookup([sub])]


def mark_base(glyphs):
	marks = {g: (i % 24, otl.buildAnchor(0, 600 + i)) for i, g in enumerate(glyphs[1:49])}
	bases = {
		g: {c: otl.buildAnchor(i, 700 + c) for c in range(24)}
		for i, g in enumerate(glyphs[49:449])
	}
	sub = otl.buildMarkBasePosSubtable(marks, bases, font_map(glyphs))
	return "GPOS", [otl.buildLookup([sub])]


def many_lookups(glyphs):
	lookups = []
	for i in range(1, 1500):
		mapping = {glyphs[(i * 13 + k) % 2999 + 1]: glyphs[(i * 7 + k * 3) % 2999 + 1] for k in range(60)}
		lookups.append(otl.buildLookup([otl.buildSingleSubstSubtable(mapping)]))
	return "GSUB", lookups


def ligatures(glyphs):
	ligs = {}
	for i in range(1, 1201):
		for k in range(12):
			ligs[(glyphs[i], glyphs[1 + (i + k) % 2999], glyphs[1 + (i * 3 + k) % 2999])] = glyphs[
				1 + (i * 5 + k) % 2999
			]
	return "GSUB", [otl.buildLookup([otl.buildLigatureSubstSubtable(ligs)])]


def multiple(glyphs):
	mapping = {g: [glyphs[1 + (i + k) % 2999] for k in range(12)] for i, g in enumerate(glyphs[1:3001])}
	return "GSUB", [otl.buildLookup([otl.buildMultipleSubstSubtable(mapping)])]


def alternates(glyphs):
	mapping = {g: [glyphs[1 + (i * 3 + k) % 2999] for k in range(12)] for i, g in enumerate(glyphs[1:3001])}
	return "GSUB", [otl.buildLookup([otl.buildAlternateSubstSubtable(mapping)])]



_maps = {}


def font_map(glyphs):
	key = len(glyphs)
	if key not in _maps:
		_maps[key] = {g: i for i, g in enumerate(glyphs)}
	return _maps[key]


CASES = {
	"pairpos-glyphs": (3000, pair_pos_glyphs),
	"pairpos-classes": (3000, pair_pos_classes),
	"singlepos": (36000, single_pos),
	"markbase": (3000, mark_base),
	"many-lookups": (3000, many_lookups),
	"ligatures": (3000, ligatures),
	"multiple": (3001, multiple),
	"alternates": (3001, alternates),
}


class Counter(logging.Handler):
	def __init__(self):
		super().__init__()
		self.count = 0

	def emit(self, record):
		if "Attempting to fix OTLOffsetOverflowError" in record.getMessage():
			self.count += 1


def main(out):
	os.makedirs(out, exist_ok=True)
	counter = Counter()
	logger = logging.getLogger("fontTools.ttLib.tables.otBase")
	logger.addHandler(counter)
	logger.setLevel(logging.INFO)
	for name, (n, make) in CASES.items():
		glyphs = glyph_names(n)
		font = base_font(glyphs)
		tag, lookups = make(glyphs)
		font[tag] = layout(tag, lookups)
		path = os.path.join(out, f"{name}.parity.ttx")
		font.saveXML(path)
		# Check that it overflows, on a copy read back from the TTX.
		from fontTools.ttLib import TTFont

		check = TTFont()
		check.cfg[REPACKER] = False
		check.importXML(path)
		counter.count = 0
		check[tag].compile(check)
		print(f"{name}: {counter.count} overflow fix(es)")


main(sys.argv[1])
