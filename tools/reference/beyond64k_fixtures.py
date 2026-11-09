"""Generates the fixtures of the tests of beyond-64k fonts.

fontTools' tests build these fonts with FontBuilder, feaLib, otlLib and varLib, which are
not ported yet. This script runs the same code as those tests and saves the results:

- crates/fontoxide-ttlib/tests/data/beyond64k: Tests/ttLib/beyond64k_test.py. The fragments
  only have the tables that the test adds to Tests/ttLib/data/TestTTF-Regular.ttx.
- crates/fontoxide-subset/tests/data: Tests/subset/subset_test.py and the subsetting and
  maxContextCalc tests of Tests/ttLib/beyond64k_test.py.
- crates/fontoxide-varlib/tests/data/instancer: Tests/varLib/instancer/instancer_test.py.

Fonts with more than 65536 glyphs are saved as WOFF2 without transformations, where they
are small because nearly all the glyphs are empty.

    uv run --project tools/reference python tools/reference/beyond64k_fixtures.py
"""

import importlib.util
import io
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "upstream" / "fonttools" / "Lib"))
sys.dont_write_bytecode = True

from fontTools import designspaceLib, varLib
from fontTools.feaLib.builder import addOpenTypeFeaturesFromString
from fontTools.fontBuilder import FontBuilder
from fontTools.otlLib import builder
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib import TTFont, newTable
from fontTools.ttLib.beyond64k import upper_tables
from fontTools.ttLib.tables import _g_l_y_f, otTables as ot
from fontTools.ttLib.tables.TupleVariation import TupleVariation
from fontTools.ttLib.woff2 import WOFF2FlavorData

TESTS = ROOT / "upstream" / "fonttools" / "Tests"
TTLIB = ROOT / "crates" / "fontoxide-ttlib" / "tests" / "data" / "beyond64k"
SUBSET = ROOT / "crates" / "fontoxide-subset" / "tests" / "data"
INSTANCER = ROOT / "crates" / "fontoxide-varlib" / "tests" / "data" / "instancer"


def load(name, relative):
    spec = importlib.util.spec_from_file_location(name, TESTS / relative)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def write_tables(font, path, tables=None):
    """Saves some tables of a font as TTX, from memory."""
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        font.saveXML(f, tables=tables, newlinestr="\n")
    print(path.name)


def write_font(font, path):
    """Saves a font as TTX, through binary as the fixtures of fontTools do."""
    font["head"].created = font["head"].modified = 0
    buf = io.BytesIO()
    font.save(buf)
    buf.seek(0)
    write_tables(TTFont(buf, recalcTimestamp=False), path)


def write_woff2(font, path):
    font.flavor = "woff2"
    font.flavorData = WOFF2FlavorData(transformedTables=[])
    font.recalcTimestamp = False
    if "head" in font:
        font["head"].created = font["head"].modified = 0
    font.save(path)
    print(path.name, path.stat().st_size)


def test_ttf():
    font = TTFont()
    font.importXML(TESTS / "ttLib" / "data" / "TestTTF-Regular.ttx")
    return font


def contextual_formats():
    """`test_convert_contextual_layout_formats`: its subtables as lookups, in the order of
    the test: formats 1, 2 and 3 of the plain ones, and formats 1 and 2 of the chaining ones.
    """
    font = TTFont()
    font.setGlyphOrder([".notdef", "a", "b", "c", "d"])
    for tag, builder_type, table_type in (
        ("GSUB", builder.ChainContextSubstBuilder, ot.GSUB),
        ("GPOS", builder.ChainContextPosBuilder, ot.GPOS),
    ):
        lookup_builder = builder_type(font, None)
        lookups = []
        for chaining in (False, True):
            lookup = builder.SingleSubstBuilder(font, None)
            lookup.lookup_index = 3
            rule = builder.ChainContextualRule(
                [["a"]] if chaining else [],
                [["b"], ["c"]],
                [["d"]] if chaining else [],
                [[lookup], None],
            )
            ruleset = builder.ChainContextualRuleset()
            ruleset.addRule(rule)
            subtables = [
                lookup_builder.buildFormat1Subtable(ruleset, chaining),
                lookup_builder.buildFormat2Subtable(
                    ruleset, ruleset.format2ClassDefs(), chaining
                ),
            ]
            if not chaining:
                subtables.append(lookup_builder.buildFormat3Subtable(rule, chaining))
            lookups.extend(
                builder.buildLookup([subtable], table=tag) for subtable in subtables
            )
        table = table_type()
        table.Version = 0x00010000
        table.ScriptList = table.FeatureList = None
        table.LookupList = ot.LookupList()
        table.LookupList.Lookup = lookups
        table.LookupList.LookupCount = len(lookups)
        font[tag] = newTable(tag)
        font[tag].table = table
    return font


def gdef_font():
    font = test_ttf()
    addOpenTypeFeaturesFromString(
        font,
        """
        table GDEF {
            GlyphClassDef [period], [ellipsis], , ;
            LigatureCaretByPos ellipsis 300;
        } GDEF;
        """,
    )
    return font


def base_font():
    font = test_ttf()
    addOpenTypeFeaturesFromString(
        font,
        """
        table BASE {
            HorizAxis.BaseTagList romn;
            HorizAxis.BaseScriptList latn romn 0;
        } BASE;
        """,
    )
    coord = (
        font["BASE"]
        .table.HorizAxis.BaseScriptList.BaseScriptRecord[0]
        .BaseScript.BaseValues.BaseCoord[0]
    )
    coord.Format = 2
    coord.ReferenceGlyph = "period"
    coord.BaseCoordPoint = 0
    return font


def layout_header_font():
    font = test_ttf()
    addOpenTypeFeaturesFromString(
        font,
        """
        feature kern { pos period period -20; } kern;
        feature liga { sub period period by ellipsis; } liga;
        """,
    )
    return font


def head_after_loca_font():
    """The font of `test_head_written_after_LOCA_with_long_loca`, without its big glyphs,
    which the test adds."""
    order = [".notdef", "A", "B", "C", "D"]
    fb = FontBuilder(unitsPerEm=1000)
    fb.setupGlyphOrder(order)
    fb.setupCharacterMap({0x41: "A"})
    fb.setupGlyf({g: _g_l_y_f.Glyph() for g in order})
    fb.setupHorizontalMetrics({g: (500, 0) for g in order})
    fb.setupHorizontalHeader(ascent=800, descent=-200)
    fb.setupNameTable({"familyName": "Test", "styleName": "Regular"})
    fb.setupOS2()
    fb.setupPost()
    fb.setupMaxp()
    return fb.font


def subset_contextual_font(upstream):
    """The font of `test_subset_extended_contextual_layout`."""
    font = upstream.build_contextual_layout_font()
    value = ot.ValueRecord()
    value.XAdvance = -20
    for tag, subtable in (
        ("GSUB", builder.buildSingleSubstSubtable({"period": "ellipsis"})),
        (
            "GPOS",
            builder.buildSinglePos({"period": value}, font.getReverseGlyphMap())[0],
        ),
    ):
        table = font[tag].table
        table.LookupList.Lookup.insert(0, builder.buildLookup([subtable], table=tag))
        table.LookupList.LookupCount = len(table.LookupList.Lookup)
        feature = table.FeatureList.FeatureRecord[0].Feature
        feature.LookupListIndex = list(range(1, table.LookupList.LookupCount))
        feature.LookupCount = len(feature.LookupListIndex)
    return font


def uppercase_companion_font(vertical):
    """`SubsetTest.test_uppercase_companion_tables`, before `upper_tables`."""
    fb = FontBuilder(1000)
    order = [".notdef", "a", "b", "unused"]
    fb.setupGlyphOrder(order)
    fb.setupCharacterMap({0x61: "a", 0x62: "b"})
    fb.setupGlyf({name: TTGlyphPen(None).glyph() for name in order})
    fb.setupHorizontalMetrics({name: (500, 0) for name in order})
    fb.setupHorizontalHeader()
    if vertical:
        fb.setupVerticalMetrics({name: (1000, 0) for name in order})
        fb.setupVerticalHeader()
    fb.setupNameTable({"familyName": "Uppercase subset", "styleName": "Regular"})
    fb.setupOS2()
    fb.setupPost()
    return fb.font


def variable_notdef_beyond64k():
    """`test_subset_variable_notdef_table_order` with `beyond64k=True`."""
    fb = FontBuilder(1000, beyond64k=True)
    order = [".notdef", "A"]
    fb.setupGlyphOrder(order)
    fb.setupCharacterMap({0x41: "A"})
    pen = TTGlyphPen(None)
    for points in (
        [(50, 0), (50, 700), (450, 700), (450, 0)],
        [(100, 50), (400, 50), (400, 650), (100, 650)],
    ):
        pen.moveTo(points[0])
        for point in points[1:]:
            pen.lineTo(point)
        pen.closePath()
    notdef = pen.glyph()
    fb.setupGlyf({".notdef": notdef, "A": notdef})
    fb.setupHorizontalMetrics({name: (500, 50) for name in order})
    fb.setupHorizontalHeader()
    fb.setupNameTable({"familyName": "Variable Notdef", "styleName": "Regular"})
    fb.setupOS2()
    fb.setupPost()
    fb.setupFvar([("wght", 100, 400, 900, "Weight")], [])
    deltas = [
        (-20, 0), (-20, 0), (20, 0), (20, 0), (-10, -5), (10, -5),
        (10, 5), (-10, 5), (0, 0), (40, 0), (0, 0), (0, 0),
    ]
    fb.setupGvar(
        {name: [TupleVariation({"wght": (0, 1, 1)}, deltas)] for name in order}
    )
    return fb.font


def extended_layout_font(extension):
    """The font of `test_subset_extended_layout`, before it is saved."""
    fb = FontBuilder(1000)
    order = [
        ".notdef", "a", "b", "c", "d", "a.alt", "b.alt", "a_b", "acute", "grave", "unused",
    ]
    fb.setupGlyphOrder(order)
    fb.setupCharacterMap(
        {
            97: "a",
            98: "b",
            99: "c",
            100: "d",
            0x301: "acute",
            0x300: "grave",
            120: "unused",
        }
    )
    fb.setupGlyf({name: TTGlyphPen(None).glyph() for name in order})
    fb.setupHorizontalMetrics({name: (500, 0) for name in order})
    fb.setupHorizontalHeader()
    fb.setupNameTable({"familyName": "Extended Layout", "styleName": "Regular"})
    fb.setupOS2()
    fb.setupPost()
    fb.addOpenTypeFeatures("""
        languagesystem DFLT dflt;
        markClass acute <anchor 0 0> @TOP;
        markClass grave <anchor 50 0> @TOP;
        feature salt { sub b from [b.alt c]; } salt;
        feature liga { sub a b by a_b; } liga;
        feature ccmp { sub d by a b; } ccmp;
        feature calt {
            lookupflag UseMarkFilteringSet [acute grave];
            sub c a' b by a.alt;
        } calt;
        feature rclt { rsub c a' b by a.alt; } rclt;
        feature kern {
            pos a -50;
            pos a b -30;
            pos [a c] [b d] -15;
        } kern;
        feature curs {
            pos cursive a <anchor 0 0> <anchor 300 0>;
            pos cursive b <anchor 0 0> <anchor 300 0>;
        } curs;
        feature mark {
            pos base a <anchor 100 300> mark @TOP;
            pos ligature a_b <anchor 100 300> mark @TOP
                ligComponent <anchor 200 300> mark @TOP;
        } mark;
        feature mkmk { pos mark acute <anchor 100 300> mark @TOP; } mkmk;
    """)
    if extension:
        for tag, extension_type, lookup_type in (
            ("GSUB", ot.ExtensionSubst, 7),
            ("GPOS", ot.ExtensionPos, 9),
        ):
            for lookup in fb.font[tag].table.LookupList.Lookup:
                wrapped = []
                for subtable in lookup.SubTable:
                    wrapper = extension_type()
                    wrapper.Format = 1
                    wrapper.ExtensionLookupType = lookup.LookupType
                    wrapper.ExtSubTable = subtable
                    wrapped.append(wrapper)
                lookup.SubTable = wrapped
                lookup.LookupType = lookup_type
    return fb.font


def high_glyph_ids_font():
    """The font of `test_subset_extended_layout_high_glyph_ids`."""
    fb = FontBuilder(1000, beyond64k=True)
    order = [".notdef"] + [f"unused{i}" for i in range(1, 0x10000)] + ["a", "b", "a_b"]
    fb.setupGlyphOrder(order)
    fb.setupCharacterMap({97: "a", 98: "b"})
    fb.setupGlyf({name: TTGlyphPen(None).glyph() for name in order})
    fb.setupHorizontalMetrics({name: (500, 0) for name in order})
    fb.setupHorizontalHeader()
    fb.setupNameTable({"familyName": "High Layout GIDs", "styleName": "Regular"})
    fb.setupOS2()
    fb.setupPost()
    fb.addOpenTypeFeatures("""
        feature liga { sub a b by a_b; } liga;
        feature kern { pos a b -30; } kern;
        table GDEF { GlyphClassDef [a b], [a_b], , ; } GDEF;
    """)
    return fb.font


def beyond64k_vf():
    """The variable font of `test_full_instance_save_beyond64k`, after `upper_tables`."""
    order = [".notdef"] + ["g%05d" % i for i in range(1, 0x10001)]  # 65537
    HI = "g65536"
    ACUTE = "g65530"

    def make_hi(scale):
        pen = TTGlyphPen(None)
        pen.moveTo((100 * scale, 0))
        pen.lineTo((100 * scale, 100 * scale))
        pen.lineTo((0, 100 * scale))
        pen.closePath()
        return pen.glyph()

    def make_master(scale):
        fb = FontBuilder(unitsPerEm=1000)
        fb.setupGlyphOrder(order)
        glyphs = {g: _g_l_y_f.Glyph() for g in order}
        glyphs[HI] = make_hi(scale)
        fb.setupGlyf(glyphs)
        fb.setupHorizontalMetrics({g: (500, 0) for g in order})
        fb.setupHorizontalHeader(ascent=800, descent=-200)
        fb.setupNameTable({"familyName": "TestBeyond64k", "styleName": "R"})
        fb.setupPost(keepGlyphNames=False)
        addOpenTypeFeaturesFromString(
            fb.font,
            "markClass %(ACUTE)s <anchor %(a)d %(a)d> @TOP;\n"
            "feature mark { pos base %(HI)s <anchor %(a)d %(a)d> mark @TOP; } mark;\n"
            "table GDEF { GlyphClassDef ,,[%(ACUTE)s],; } GDEF;\n"
            % dict(ACUTE=ACUTE, HI=HI, a=10 * scale),
        )
        return fb.font

    doc = designspaceLib.DesignSpaceDocument()
    doc.addAxis(
        designspaceLib.AxisDescriptor(
            tag="wght", name="Weight", minimum=0, default=0, maximum=1000
        )
    )
    for loc, scale in [(0, 1), (1000, 2)]:
        doc.addSource(
            designspaceLib.SourceDescriptor(
                font=make_master(scale), location={"Weight": loc}
            )
        )
    vf, _, _ = varLib.build(doc)
    assert vf.hasExtendedGlyphIDs()
    upper_tables(vf)
    assert vf["GVAR"].variations[HI]
    assert vf["GDEF"].table.Version == 0x00010004
    assert vf["GDEF"].table.VarStore is not None
    return vf


def extended_layout_header_gpos(instancer, lookup_variations):
    """The GPOS case of `test_extended_layout_header`."""
    if lookup_variations:
        font = instancer.makeLookupVariationsFont()
    else:
        font = instancer.makeFeatureVarsFont([([{"wght": (0.5, 1.0)}], {"a": "a.alt"})])
    variations = font["GSUB"].table.FeatureVariations
    base, conditional = ("A", "B") if lookup_variations else ("a", "a.alt")
    addOpenTypeFeaturesFromString(
        font,
        f"""
        lookup Default {{ pos {base} 10; }} Default;
        lookup Conditional {{ pos {conditional} 20; }} Conditional;
        feature kern {{ lookup Default; }} kern;
        feature zzzz {{ lookup Conditional; }} zzzz;
        """,
        tables=["GPOS"],
    )
    del font["GSUB"]
    font["GPOS"].table.FeatureVariations = variations
    font["GPOS"].table.Version = 0x00010001
    if lookup_variations:
        lookups = variations.LookupVariationRecord[0].FeatureLookupsTable
        lookups.LookupConditionRecord = [
            instancer._lookupVariationRecord(
                instancer._lookupVariationCondition(0.25, 0.5), [1]
            )
        ]
        lookups.LookupConditionCount = 1
    else:
        for record in variations.FeatureVariationRecord:
            for substitution in record.FeatureTableSubstitution.SubstitutionRecord:
                substitution.Feature.LookupListIndex = [1]
                substitution.Feature.LookupCount = 1
    return font


def main():
    TTLIB.mkdir(parents=True, exist_ok=True)
    beyond64k_test = load("beyond64k_test", "ttLib/beyond64k_test.py")
    instancer_test = load("instancer_test", "varLib/instancer/instancer_test.py")

    write_tables(contextual_formats(), TTLIB / "contextual_formats.ttx")
    layout = beyond64k_test.build_contextual_layout_font()
    write_tables(layout, TTLIB / "contextual_layout.ttx", ["GSUB", "GPOS"])
    write_tables(gdef_font(), TTLIB / "gdef.ttx", ["GDEF"])
    write_tables(base_font(), TTLIB / "base.ttx", ["BASE"])
    write_tables(layout_header_font(), TTLIB / "layout_header.ttx", ["GSUB", "GPOS"])
    write_font(head_after_loca_font(), TTLIB / "head_after_loca.ttx")
    woff2_test = load("woff2_test", "ttLib/woff2_test.py")
    fontfile = (
        getattr(woff2_test.fontfile, "__wrapped__", None)
        or woff2_test.fontfile.__pytest_wrapped__.obj
    )
    write_tables(
        TTFont(fontfile(), recalcBBoxes=False, recalcTimestamp=False),
        TTLIB / "woff2_fontfile.ttx",
    )

    write_tables(layout, SUBSET / "beyond64k_contextual_layout.ttx", ["GSUB", "GPOS"])
    write_tables(
        subset_contextual_font(beyond64k_test),
        SUBSET / "beyond64k_subset_contextual.ttx",
        ["GSUB", "GPOS"],
    )
    for vertical in (False, True):
        write_font(
            uppercase_companion_font(vertical),
            SUBSET / f"uppercase_companion_{int(vertical)}.ttx",
        )
    write_font(variable_notdef_beyond64k(), SUBSET / "variable_notdef_beyond64k.ttx")
    for extension in (False, True):
        write_font(
            extended_layout_font(extension), SUBSET / f"extended_layout_{int(extension)}.ttx"
        )
    write_woff2(high_glyph_ids_font(), SUBSET / "high_glyph_ids.woff2")

    write_tables(
        instancer_test.makeFeatureVarsFont([([{"wght": (0.5, 1.0)}], {"a": "a.alt"})]),
        INSTANCER / "extended_layout_gsub_0.ttx",
    )
    for lookup_variations in (False, True):
        write_tables(
            extended_layout_header_gpos(instancer_test, lookup_variations),
            INSTANCER / f"extended_layout_gpos_{int(lookup_variations)}.ttx",
        )
    write_woff2(beyond64k_vf(), INSTANCER / "beyond64k_vf.woff2")


if __name__ == "__main__":
    main()
