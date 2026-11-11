"""Generates the fixtures of the tests of reorderGlyphs and scaleUpem.

fontTools' Tests/ttLib/reorderGlyphs_test.py, Tests/ttLib/scaleUpem_test.py and the
reordering tests of Tests/ttLib/beyond64k_test.py build their fonts with FontBuilder,
feaLib and otlLib, which fontoxide-ttlib cannot use in its tests. This script runs the
same code and saves:

- crates/fontoxide-ttlib/tests/data/reorder_glyphs:
  - ligatures.ttx: the font of `test_reorder_lazy_ligatures`.
  - single_pos.ttx, pair_pos.ttx: the fonts of `test_reorder_lazy_arrays`.
  - extended_layout.ttx: the font of `test_reorder_extended_layout`, and
    extended_layout.expected.ttx its GDEF, GSUB and GPOS after `reorderGlyphs`.
  - extended_contextual_layout.ttx: the GSUB and GPOS of
    `test_reorder_extended_contextual_layout` (on Tests/ttLib/data/TestTTF-Regular.ttx),
    and extended_contextual_layout.expected.ttx the tables after `reorderGlyphs`.
- crates/fontoxide-ttlib/tests/data/scale_upem:
  - cff_no_font_matrix.otf: the font of `test_scale_upem_cff_default_font_matrix`, and
    cff_no_font_matrix.expected.ttx its CFF table after `scale_and_roundtrip`.
  - math.ttx: the MATH table of `test_scale_upem_math` (on Tests/ttLib/data/I.ttf), and
    math.expected.ttx the table after `scale_and_roundtrip`.

    uv run --project tools/reference python tools/reference/reorder_scale_fixtures.py
"""

import io
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "upstream" / "fonttools" / "Lib"))
sys.dont_write_bytecode = True

from fontTools.feaLib.builder import addOpenTypeFeaturesFromString
from fontTools.fontBuilder import FontBuilder
from fontTools.otlLib import builder
from fontTools.otlLib.builder import buildMathTable
from fontTools.pens.t2CharStringPen import T2CharStringPen
from fontTools.ttLib import TTFont
from fontTools.ttLib.beyond64k import _convert_layout_formats, upper_tables
from fontTools.ttLib.reorderGlyphs import reorderGlyphs
from fontTools.ttLib.scaleUpem import scale_upem

TESTS = ROOT / "upstream" / "fonttools" / "Tests"
DATA = ROOT / "crates" / "fontoxide-ttlib" / "tests" / "data"
REORDER = DATA / "reorder_glyphs"
SCALE = DATA / "scale_upem"


def write_tables(font, path, tables=None):
    """Saves some tables of a font as TTX, from memory."""
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        font.saveXML(f, tables=tables, newlinestr="\n")
    print(path.name)


def roundtrip(font):
    stream = io.BytesIO()
    font.save(stream)
    return TTFont(io.BytesIO(stream.getvalue()), recalcTimestamp=False)


def write_font(font, path):
    """Saves a font as TTX, through binary as the fixtures of fontTools do."""
    font["head"].created = font["head"].modified = 0
    font.recalcTimestamp = False
    write_tables(roundtrip(font), path)


def ligatures():
    fb = FontBuilder(1000)
    fb.setupGlyphOrder([".notdef", "a", "b", "a_b", "unused"])
    fb.setupPost()
    addOpenTypeFeaturesFromString(fb.font, "feature liga { sub a b by a_b; } liga;")
    return fb.font


def lazy_arrays(pairpos):
    fb = FontBuilder(1000)
    order = [".notdef"] + [f"g{i}" for i in range(10)]
    fb.setupGlyphOrder(order)
    fb.setupPost()
    if pairpos:
        statements = [f"pos g0 g{i} {i + 1};" for i in range(10)]
    else:
        statements = [f"pos g{i} {i + 1};" for i in range(10)]
    addOpenTypeFeaturesFromString(
        fb.font, "feature kern { " + " ".join(statements) + " } kern;"
    )
    return fb.font


def extended_layout():
    fb = FontBuilder(1000)
    order = [".notdef", "a", "b", "c", "d", "a_b", "acute", "grave"]
    fb.setupGlyphOrder(order)
    fb.setupPost()
    addOpenTypeFeaturesFromString(
        fb.font,
        """
        languagesystem DFLT dflt;
        markClass acute <anchor 0 0> @TOP;
        markClass grave <anchor 50 0> @TOP;
        feature liga { sub a b by a_b; } liga;
        feature rclt { rsub c [a b]' d by [b a]; } rclt;
        feature kern {
            pos a -50;
            pos b -60;
            pos a [b c] -30;
            pos b [a d] -40;
            pos [a c] [b d] -15;
        } kern;
        feature curs {
            pos cursive a <anchor 0 0> <anchor 300 0>;
            pos cursive b <anchor 0 10> <anchor 400 10>;
        } curs;
        feature mark {
            pos base a <anchor 100 300> mark @TOP;
            pos base b <anchor 200 400> mark @TOP;
            pos ligature a_b <anchor 100 300> mark @TOP
                ligComponent <anchor 200 300> mark @TOP;
        } mark;
        feature mkmk {
            pos mark acute <anchor 100 300> mark @TOP;
            pos mark grave <anchor 200 400> mark @TOP;
        } mkmk;
        table GDEF {
            LigatureCaretByPos a_b 100;
            LigatureCaretByPos d 200;
        } GDEF;
        """,
    )
    return fb.font


def test_ttf():
    font = TTFont()
    font.importXML(TESTS / "ttLib" / "data" / "TestTTF-Regular.ttx")
    return font


def extended_contextual_layout():
    """`build_contextual_layout_font` with the lookups of
    `test_reorder_extended_contextual_layout`."""
    font = test_ttf()
    addOpenTypeFeaturesFromString(
        font,
        """
        feature calt { sub period by ellipsis; } calt;
        feature kern { pos period -20; } kern;
        """,
    )
    lookup = builder.SingleSubstBuilder(font, None)
    lookup.lookup_index = 0
    rule = builder.ChainContextualRule(
        [[".null", "CR"]],
        [["period", "ellipsis"], ["space"]],
        [[".null", "CR"]],
        [[lookup], None],
    )
    ruleset = builder.ChainContextualRuleset()
    ruleset.addRule(rule)
    for tag, builder_type in (
        ("GSUB", builder.ChainContextSubstBuilder),
        ("GPOS", builder.ChainContextPosBuilder),
    ):
        lookup_builder = builder_type(font, None)
        subtables = []
        for chaining in (False, True):
            subtables.extend(
                [
                    lookup_builder.buildFormat1Subtable(ruleset, chaining),
                    lookup_builder.buildFormat2Subtable(
                        ruleset, ruleset.format2ClassDefs(), chaining
                    ),
                    lookup_builder.buildFormat3Subtable(rule, chaining),
                ]
            )
        font[tag].table.LookupList.Lookup = [
            builder.buildLookup([st], table=tag) for st in subtables
        ]
    return font


def reversed_order(font):
    order = font.getGlyphOrder()
    return order[:1] + list(reversed(order[1:]))


def cff_no_font_matrix():
    fb = FontBuilder(1000, isTTF=False)
    fb.setupGlyphOrder([".notdef", "A"])
    fb.setupCharacterMap({0x41: "A"})
    pen = T2CharStringPen(600, None)
    pen.moveTo((100, 0))
    pen.lineTo((500, 0))
    pen.lineTo((300, 700))
    pen.closePath()
    charString = pen.getCharString()
    fb.setupCFF("Test-Regular", {}, {".notdef": charString, "A": charString}, {})
    fb.setupHorizontalMetrics({".notdef": (600, 100), "A": (600, 100)})
    fb.setupHorizontalHeader(ascent=800, descent=-200)
    fb.setupNameTable({"familyName": "Test", "styleName": "Regular"})
    fb.setupOS2()
    fb.setupPost()
    fb.font["head"].created = fb.font["head"].modified = 0
    fb.font.recalcTimestamp = False
    return fb.font


def scale_and_roundtrip(font):
    scale_upem(font, font["head"].unitsPerEm * 2)
    return roundtrip(font)


def math_font():
    font = TTFont(TESTS / "ttLib" / "data" / "I.ttf", recalcTimestamp=False)
    notdef, glyph = font.getGlyphOrder()
    buildMathTable(
        font,
        constants={
            "ScriptPercentScaleDown": 80,
            "ScriptScriptPercentScaleDown": 60,
            "DelimitedSubFormulaMinHeight": 1500,
            "DisplayOperatorMinHeight": 1300,
            "RadicalDegreeBottomRaisePercent": 55,
        },
        minConnectorOverlap=50,
        vertGlyphVariants={glyph: [(glyph, 1000), (notdef, 1400)]},
        vertGlyphAssembly={glyph: ([(notdef, 1, 20, 30, 400)], 0)},
    )
    return font


def main():
    REORDER.mkdir(parents=True, exist_ok=True)
    SCALE.mkdir(parents=True, exist_ok=True)

    write_font(ligatures(), REORDER / "ligatures.ttx")
    write_font(lazy_arrays(False), REORDER / "single_pos.ttx")
    write_font(lazy_arrays(True), REORDER / "pair_pos.ttx")

    font = extended_layout()
    write_font(font, REORDER / "extended_layout.ttx")
    reference = roundtrip(font)
    reorderGlyphs(reference, reversed_order(reference))
    write_tables(
        reference, REORDER / "extended_layout.expected.ttx", ["GDEF", "GSUB", "GPOS"]
    )

    font = extended_contextual_layout()
    reference = roundtrip(font)
    write_tables(reference, REORDER / "extended_contextual_layout.ttx", ["GSUB", "GPOS"])
    reorderGlyphs(reference, reversed_order(reference))
    write_tables(
        reference,
        REORDER / "extended_contextual_layout.expected.ttx",
        ["GSUB", "GPOS"],
    )

    font = cff_no_font_matrix()
    path = SCALE / "cff_no_font_matrix.otf"
    font.save(path)
    print(path.name)
    font = scale_and_roundtrip(TTFont(path, recalcTimestamp=False))
    write_tables(font, SCALE / "cff_no_font_matrix.expected.ttx", ["CFF "])

    font = math_font()
    write_tables(font, SCALE / "math.ttx", ["MATH"])
    font = scale_and_roundtrip(font)
    write_tables(font, SCALE / "math.expected.ttx", ["MATH"])


if __name__ == "__main__":
    main()
