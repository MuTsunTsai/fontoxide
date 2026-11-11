"""Generates the fixtures of crates/fontoxide-ttlib/tests/tables_graphite.rs.

The only Graphite font among fontTools' test data, Tests/ttLib/tables/data/graphite/
graphite_tests.ttf, has version 2 `Silf`, version 1 `Glat` and version 2 `Feat`
tables, without compression, pseudo-glyphs, non-linear classes or constraints. This
script modifies its Graphite tables in memory to cover the other versions and
features, and saves for each variant <name>:

- <name>.ttf: the font, as compiled by fontTools.
- <name>.ttx: the Graphite tables of <name>.ttf as TTX; or <name>.ttx.err with the
  error if fontTools fails to decompile them (then there is nothing else).
- <name>.roundtrip.ttf: <name>.ttf read, its Graphite tables decompiled, and saved;
  or <name>.roundtrip.err with the error if fontTools fails to.
- <name>.fromttx.ttf: <name>.ttf without its Graphite tables, with <name>.ttx
  imported, and saved; or <name>.fromttx.err.

fontTools cannot compile `Silf` subtables with critical features (it misspells
`numCritFeatures`) nor version 3 `Glat` tables without octaboxes (it adds bytes to a
`str`); fontoxide can (LENIENT). Both are patched here so that fontTools produces
what fontoxide is expected to.

Compressed tables (scheme 1) are compressed with LZ4 HC by fontTools and with
lz4_flex by fontoxide; the tests compare them decompressed.

    uv run --project tools/reference python tools/reference/graphite_fixtures.py
"""

import io
import pathlib
import sys
from array import array

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "upstream" / "fonttools" / "Lib"))
sys.dont_write_bytecode = True

from fontTools.ttLib import TTFont
from fontTools.ttLib.tables import G__l_a_t, S__i_l_f

SOURCE = (
	ROOT / "upstream" / "fonttools" / "Tests" / "ttLib" / "tables" / "data" / "graphite"
	/ "graphite_tests.ttf"
)
OUT = ROOT / "crates" / "fontoxide-ttlib" / "tests" / "data" / "graphite"
GRAPHITE = ["Feat", "Glat", "Gloc", "Silf", "Sill"]


class Obj:
	def __init__(self, **kwargs):
		self.__dict__.update(kwargs)


# --- Patches for the two crashes (see the docstring) ---------------------------

_silf_compile = S__i_l_f.Silf.compile


def silf_compile(self, ttFont, version=2.0):
	self.numCritFeaturs = len(self.critFeatures)
	return _silf_compile(self, ttFont, version)


S__i_l_f.Silf.compile = silf_compile

_compile3 = G__l_a_t.table_G__l_a_t.compileAttributes3


def compile_attributes3(self, attrs):
	if self.hasOctaboxes:
		return _compile3(self, attrs)
	return self.compileAttributes12(attrs, G__l_a_t.Glat_format_23_entry)


G__l_a_t.table_G__l_a_t.compileAttributes3 = compile_attributes3


# --- Variants ------------------------------------------------------------------


def load():
	return TTFont(SOURCE, recalcTimestamp=False)


def enrich_silf(font, version, compiler_version=7, scheme=0, pseudo=True):
	silf = font["Silf"]
	silf.version = version
	silf.compilerVersion = compiler_version
	silf.scheme = scheme
	s = silf.silfs[0]
	s.ruleVersion = 1.5
	if pseudo:
		s.pMap = {0x41: "a", 0x1F600 if version >= 3.0 else 0x2000: "b"}
	s.scriptTags = ["latn", "cyrl"]
	s.critFeatures = [1, 2]
	s.jLevels = [
		Obj(attrStretch=1, attrShrink=2, attrStep=3, attrWeight=4, runto=5),
		Obj(attrStretch=6, attrShrink=7, attrStep=8, attrWeight=9, runto=10),
	]
	s.classes.nonLinear = [{"b": 1, "a": 0, "space": 2}, {"c": 0}]
	p0, p1 = s.passes
	p0.passConstraints = S__i_l_f.assemble(["PUSH_BYTE(1)", "PUSH_SHORT(-300)", "POP_RET"])
	p1.ruleConstraints[0] = S__i_l_f.assemble(["PUSH_GLYPH_ATTR(300, -1)", "RET_TRUE"])
	p1.actions[0] += S__i_l_f.assemble(["PUT_SUBS(-1, 2, 3)", "BITSET(1, 2)", "ASSOC(1, 2, 3)"])


def octaboxes(font, scheme):
	glat = font["Glat"]
	glat.version = 3.0
	glat.scheme = scheme
	glat.hasOctaboxes = True
	for i, attrs in enumerate(glat.attributes.values()):
		bitmap = [0, 0x0003, 0x8001, 0x0010, 0xFFFF][i % 5]
		attrs.octabox = Obj(
			subboxBitmap=bitmap,
			diagNegMin=10 + i,
			diagNegMax=200,
			diagPosMin=0,
			diagPosMax=255 - i,
			subboxes=[
				Obj(
					left=j,
					right=128 + j,
					bottom=3 * j,
					top=250,
					diagNegMin=j,
					diagNegMax=17,
					diagPosMin=99,
					diagPosMax=100 + j,
				)
				for j in range(bin(bitmap).count("1"))
			],
		)


def numeric_feat(font):
	feat = font["Feat"]
	feat.version = 1.0
	features = list(feat.features.values())
	feat.features = {str(i + 1): f for i, f in enumerate(features)}


def more_langs(font):
	sill = font["Sill"]
	sill.langs["en"] = [(0x6E6F6D20, 1), (1, 0)]
	sill.langs["zh-Hant"] = [(0x7965736D, 0)]


def gloc_ids(font):
	gloc = font["Gloc"]
	gloc.attribIds = array("H", range(10, 10 + gloc.numAttribs))


def variant_base(font):
	pass


def variant_v3(font):
	enrich_silf(font, 3.0)
	font["Glat"].version = 2.0
	gloc_ids(font)
	numeric_feat(font)
	more_langs(font)


def variant_v4(font):
	enrich_silf(font, 4.0)
	octaboxes(font, 0)


def variant_v5(font):
	enrich_silf(font, 5.0, scheme=1)
	octaboxes(font, 1)
	# Alphabetic feature IDs make version 1 heal itself to version 2.
	font["Feat"].version = 1.0
	more_langs(font)


def variant_v5_uncompressed(font):
	enrich_silf(font, 5.0, scheme=0)
	glat = font["Glat"]
	glat.version = 3.0
	glat.hasOctaboxes = False
	glat.scheme = 0


def variant_v2_pseudo(font):
	s = font["Silf"].silfs[0]
	s.pMap = {0x41: "a"}


VARIANTS = {
	"base": variant_base,
	"v3": variant_v3,
	"v4": variant_v4,
	"v5": variant_v5,
	"v5_uncompressed": variant_v5_uncompressed,
	"v2_pseudo": variant_v2_pseudo,
}


def save(font, path):
	"""Saves the font, or the error to <path>.err."""
	err = path.with_suffix(".err")
	for p in (path, err):
		p.unlink(missing_ok=True)
	stream = io.BytesIO()
	try:
		font.save(stream)
	except Exception as e:
		err.write_text(f"{type(e).__name__}: {e}\n", encoding="utf-8")
		return
	path.write_bytes(stream.getvalue())


def main():
	OUT.mkdir(exist_ok=True)
	for name, build in VARIANTS.items():
		font = load()
		for tag in GRAPHITE:
			font[tag]
		build(font)
		path = OUT / f"{name}.ttf"
		save(font, path)

		font = TTFont(path, recalcTimestamp=False)
		ttx = OUT / f"{name}.ttx"
		err = OUT / f"{name}.ttx.err"
		err.unlink(missing_ok=True)
		try:
			font.saveXML(io.StringIO(), tables=GRAPHITE, newlinestr="\n")
		except Exception as e:
			ttx.unlink(missing_ok=True)
			err.write_text(f"{type(e).__name__}: {e}\n", encoding="utf-8")
			print(name, "(fails to decompile)")
			continue
		with open(ttx, "w", encoding="utf-8", newline="\n") as f:
			font.saveXML(f, tables=GRAPHITE, newlinestr="\n")

		font = TTFont(path, recalcTimestamp=False)
		for tag in GRAPHITE:
			font[tag]
		save(font, OUT / f"{name}.roundtrip.ttf")

		font = TTFont(path, recalcTimestamp=False)
		for tag in GRAPHITE:
			del font[tag]
		font.importXML(ttx)
		save(font, OUT / f"{name}.fromttx.ttf")
		print(name)


if __name__ == "__main__":
	main()
