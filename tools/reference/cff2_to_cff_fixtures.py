"""Generates the fixtures of crates/fontoxide-ttlib/tests/cff2_to_cff.rs.

Tests/cffLib/cffLib_test.py builds its fonts with FontBuilder and T2CharStringPen, which are
not ported yet. This script runs the same code as _makeCFF2Font, and saves the fonts as TTX,
and also the CFF table that convertCFF2ToCFF makes of them.

    uv run --project tools/reference python tools/reference/cff2_to_cff_fixtures.py
"""

import io
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "upstream" / "fonttools" / "Lib"))
sys.dont_write_bytecode = True

from fontTools.cffLib import FDSelect
from fontTools.cffLib.CFF2ToCFF import convertCFF2ToCFF
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.t2CharStringPen import T2CharStringPen
from fontTools.ttLib import TTFont

OUT = ROOT / "crates" / "fontoxide-ttlib" / "tests" / "data"


def make_cff2_font(glyph_order, fd_select=None):
    fb = FontBuilder(1000, isTTF=False)
    fb.setupGlyphOrder(glyph_order)
    fb.setupCharacterMap({})
    char_strings = {}
    for i, glyph_name in enumerate(glyph_order):
        pen = T2CharStringPen(None, None, CFF2=True)
        pen.moveTo((0, 0))
        pen.lineTo((100 + i * 10, 0))
        pen.lineTo((0, 100))
        pen.closePath()
        char_strings[glyph_name] = pen.getCharString()
    fd_array_list = None
    if fd_select is not None:
        fd_array_list = [{} for _ in range(max(fd_select) + 1)]
    fb.setupCFF2(char_strings, fdArrayList=fd_array_list)
    if fd_select is not None:
        top_dict = fb.font["CFF2"].cff[0]
        top_dict.FDSelect = FDSelect()
        top_dict.FDSelect.format = 0
        top_dict.FDSelect.gidArray = fd_select
    fb.setupHorizontalMetrics(
        {glyph_name: (500 + i, 0) for i, glyph_name in enumerate(glyph_order)}
    )
    fb.setupHorizontalHeader()
    fb.setupNameTable({"familyName": "TestCFF2", "styleName": "Regular"})
    fb.setupOS2()
    fb.setupPost()
    fb.font["head"].created = fb.font["head"].modified = 0
    buf = io.BytesIO()
    fb.font.save(buf)
    buf.seek(0)
    return TTFont(buf, recalcBBoxes=False, recalcTimestamp=False)


def write(font, name, **kwargs):
    with open(OUT / f"{name}.ttx", "w", encoding="utf-8", newline="\n") as f:
        font.saveXML(f, newlinestr="\n", **kwargs)
    print(name)


def fixture(name, *args):
    font = make_cff2_font(*args)
    write(font, name)
    convertCFF2ToCFF(font)
    buf = io.BytesIO()
    font.save(buf)
    buf.seek(0)
    write(TTFont(buf, recalcTimestamp=False), f"{name}.converted", tables=["CFF "])


def main():
    fixture("cff2_single_fontdict", [".notdef", "A", "B"])
    fixture("cff2_multiple_fontdicts", [".notdef", "A", "B"], [0, 1, 1])


if __name__ == "__main__":
    main()
