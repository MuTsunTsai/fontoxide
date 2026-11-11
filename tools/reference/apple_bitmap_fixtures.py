"""Generates the fixtures of crates/fontoxide-ttlib/tests/tables_apple_bitmap.rs.

fontTools has no test font with Apple's bitmap tables `bdat`, `bloc` and `bhed`
(its `_b_d_a_t`, `_b_l_o_c` and `_b_h_e_d`, which are `EBDT`, `EBLC` and `head`
under other tags). This script builds a small font with `EBLC`/`EBDT` strikes
of several index and image formats, copies those tables and `head` as raw data
to `bloc`, `bdat` and `bhed`, and saves what fontTools does with the result,
in crates/fontoxide-ttlib/tests/data/apple_bitmap:

- apple.ttf: the font (the copied tables were never decompiled).
- apple.ttx, apple.row.ttx: `bhed`, `bloc` and `bdat` dumped to TTX, with the
  `raw` and `row` bitmap glyph data formats.
- apple.recompiled.ttf: the font saved with those tables decompiled.
- apple.fromttx.ttf: the font saved after importing apple.ttx into it.

All fonts are saved without recalculating the timestamp.

    uv run --project tools/reference python tools/reference/apple_bitmap_fixtures.py
"""

import io
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "upstream" / "fonttools" / "Lib"))
sys.dont_write_bytecode = True

from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib import TTFont
from fontTools.ttLib.tables.DefaultTable import DefaultTable

OUT = ROOT / "crates" / "fontoxide-ttlib" / "tests" / "data" / "apple_bitmap"

GLYPHS = [".notdef", "A", "B", "C", "D", "E"]

LINE_METRICS = """\
        <sbitLineMetrics direction="{direction}">
          <ascender value="{ascender}"/>
          <descender value="-2"/>
          <widthMax value="{ppem}"/>
          <caretSlopeNumerator value="1"/>
          <caretSlopeDenominator value="0"/>
          <caretOffset value="0"/>
          <minOriginSB value="0"/>
          <minAdvanceSB value="0"/>
          <maxBeforeBL value="{ascender}"/>
          <minAfterBL value="-2"/>
          <pad1 value="0"/>
          <pad2 value="0"/>
        </sbitLineMetrics>
"""


def size_table(ppem, start, end):
	return (
		"      <bitmapSizeTable>\n"
		+ LINE_METRICS.format(direction="hori", ascender=ppem - 2, ppem=ppem)
		+ LINE_METRICS.format(direction="vert", ascender=ppem - 2, ppem=ppem)
		+ f"""\
        <colorRef value="0"/>
        <startGlyphIndex value="{start}"/>
        <endGlyphIndex value="{end}"/>
        <ppemX value="{ppem}"/>
        <ppemY value="{ppem}"/>
        <bitDepth value="1"/>
        <flags value="1"/>
      </bitmapSizeTable>
"""
	)


BIG_METRICS = """\
        <BigGlyphMetrics>
          <height value="{h}"/>
          <width value="{w}"/>
          <horiBearingX value="0"/>
          <horiBearingY value="{h}"/>
          <horiAdvance value="{a}"/>
          <vertBearingX value="0"/>
          <vertBearingY value="0"/>
          <vertAdvance value="{a}"/>
        </BigGlyphMetrics>
"""

SMALL_METRICS = """\
        <SmallGlyphMetrics>
          <height value="{h}"/>
          <width value="{w}"/>
          <BearingX value="0"/>
          <BearingY value="{h}"/>
          <Advance value="{a}"/>
        </SmallGlyphMetrics>
"""


def index_sub_table(index_format, image_format, names, extra=""):
	gids = [GLYPHS.index(n) for n in names]
	locs = "".join(f'        <glyphLoc id="{g}" name="{n}"/>\n' for g, n in zip(gids, names))
	return (
		f'      <eblc_index_sub_table_{index_format} imageFormat="{image_format}" '
		f'firstGlyphIndex="{min(gids)}" lastGlyphIndex="{max(gids)}">\n'
		+ extra
		+ locs
		+ f"      </eblc_index_sub_table_{index_format}>\n"
	)


def glyph(image_format, name, data, metrics=""):
	return (
		f'      <ebdt_bitmap_format_{image_format} name="{name}">\n'
		+ metrics
		+ f"        <rawimagedata>\n          {data}\n        </rawimagedata>\n"
		+ f"      </ebdt_bitmap_format_{image_format}>\n"
	)


def bitmap_ttx():
	constant = '        <imageSize value="4"/>\n' + BIG_METRICS.format(h=4, w=8, a=9)
	eblc = (
		'  <EBLC>\n    <header version="2.0"/>\n'
		+ '    <strike index="0">\n'
		+ size_table(8, 1, 4)
		+ index_sub_table(1, 1, ["A", "B"])
		+ index_sub_table(2, 5, ["C", "D"], constant)
		+ "    </strike>\n"
		+ '    <strike index="1">\n'
		+ size_table(16, 1, 5)
		+ index_sub_table(3, 2, ["A", "C"])
		+ index_sub_table(4, 6, ["B", "D"])
		+ index_sub_table(5, 5, ["E"], constant)
		+ "    </strike>\n"
		+ "  </EBLC>\n"
	)
	ebdt = (
		'  <EBDT>\n    <header version="2.0"/>\n'
		+ '    <strikedata index="0">\n'
		+ glyph(1, "A", "183c 6642", SMALL_METRICS.format(h=2, w=8, a=9))
		+ glyph(1, "B", "fc82 fc82 fc", SMALL_METRICS.format(h=5, w=7, a=8))
		+ glyph(5, "C", "3c42 403c")
		+ glyph(5, "D", "f888 88f8")
		+ "    </strikedata>\n"
		+ '    <strikedata index="1">\n'
		+ glyph(2, "A", "5a5a 5a", SMALL_METRICS.format(h=3, w=7, a=8))
		+ glyph(2, "C", "f0", SMALL_METRICS.format(h=2, w=4, a=5))
		+ glyph(6, "B", "ff81 81ff", BIG_METRICS.format(h=4, w=8, a=9))
		+ glyph(6, "D", "c3c3", BIG_METRICS.format(h=2, w=8, a=9))
		+ glyph(5, "E", "0102 0408")
		+ "    </strikedata>\n"
		+ "  </EBDT>\n"
	)
	return '<?xml version="1.0" encoding="UTF-8"?>\n<ttFont>\n' + eblc + ebdt + "</ttFont>\n"


def build():
	fb = FontBuilder(1000, isTTF=True)
	fb.setupGlyphOrder(GLYPHS)
	fb.setupCharacterMap({0x41: "A", 0x42: "B", 0x43: "C", 0x44: "D", 0x45: "E"})
	pen = TTGlyphPen(None)
	pen.moveTo((0, 0))
	pen.lineTo((0, 500))
	pen.lineTo((500, 500))
	pen.closePath()
	square = pen.glyph()
	fb.setupGlyf({name: square for name in GLYPHS})
	fb.setupHorizontalMetrics({name: (600, 0) for name in GLYPHS})
	fb.setupHorizontalHeader(ascent=800, descent=-200)
	fb.setupNameTable({"familyName": "Apple Bitmap", "styleName": "Regular"})
	fb.setupOS2()
	fb.setupPost()
	fb.font.importXML(io.StringIO(bitmap_ttx()))
	fb.font["head"].created = fb.font["head"].modified = 3_000_000_000
	buf = io.BytesIO()
	fb.save(buf)

	# Copy the tables as raw data to their Apple tags.
	font = TTFont(io.BytesIO(buf.getvalue()), recalcTimestamp=False)
	for old, new, keep in [("EBLC", "bloc", False), ("EBDT", "bdat", False), ("head", "bhed", True)]:
		table = DefaultTable(new)
		table.data = font.reader[old]
		font[new] = table
		if not keep:
			del font[old]
	buf = io.BytesIO()
	font.save(buf)
	return buf.getvalue()


def main():
	OUT.mkdir(parents=True, exist_ok=True)
	data = build()
	(OUT / "apple.ttf").write_bytes(data)
	tables = ["bhed", "bloc", "bdat"]
	for name, fmt in [("apple.ttx", "raw"), ("apple.row.ttx", "row")]:
		font = TTFont(io.BytesIO(data))
		font.saveXML(OUT / name, tables=tables, bitmapGlyphDataFormat=fmt)

	font = TTFont(io.BytesIO(data), recalcTimestamp=False)
	for tag in tables:
		font[tag]
	font.save(OUT / "apple.recompiled.ttf")

	font = TTFont(io.BytesIO(data), recalcTimestamp=False)
	font.importXML(OUT / "apple.ttx")
	font.save(OUT / "apple.fromttx.ttf")


if __name__ == "__main__":
	main()
