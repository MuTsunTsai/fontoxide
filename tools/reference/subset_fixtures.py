"""Generates the fixtures of crates/fontoxide-subset/tests/data.

fontTools' Tests/subset builds some of its test fonts with FontBuilder and feaLib,
which are not ported yet. This script runs the same code as the fixtures of
Tests/subset/subset_test.py and saves the resulting fonts as TTX.

    uv run --project tools/reference python tools/reference/subset_fixtures.py
"""

import io
import pathlib
import sys
from copy import deepcopy
from string import ascii_letters

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "upstream" / "fonttools" / "Lib"))
sys.dont_write_bytecode = True

from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib import TTFont, newTable
from fontTools.ttLib.tables import otTables as ot
from fontTools.ttLib.tables.TupleVariation import TupleVariation
from fontTools.varLib.builder import buildVarData, buildVarRegionList, buildVarStore

OUT = ROOT / "crates" / "fontoxide-subset" / "tests" / "data"
OTLLIB = ROOT / "crates" / "fontoxide-otllib" / "tests" / "data"


def roundtrip(font):
    buf = io.BytesIO()
    font.save(buf)
    buf.seek(0)
    return TTFont(buf, recalcTimestamp=False)


def write(font, name):
    """Saves a font as TTX, through binary as the fixtures of fontTools do."""
    if "head" in font:
        font["head"].created = font["head"].modified = 0
    font = roundtrip(font)
    with open(OUT / f"{name}.ttx", "w", encoding="utf-8", newline="\n") as f:
        font.saveXML(f, newlinestr="\n")
    print(name)


def feature_vars_font():
    fb = FontBuilder(unitsPerEm=100)
    fb.setupGlyphOrder([".notdef", "f", "f_f", "dollar", "dollar.rvrn"])
    fb.setupCharacterMap({ord("f"): "f", ord("$"): "dollar"})
    fb.setupNameTable({"familyName": "TestFeatureVars", "styleName": "Regular"})
    fb.setupPost()
    fb.setupFvar(axes=[("wght", 100, 400, 900, "Weight")], instances=[])
    fb.addOpenTypeFeatures("""\
        feature dlig {
            sub f f by f_f;
        } dlig;
    """)
    fb.addFeatureVariations(
        [([{"wght": (0.20886, 1.0)}], {"dollar": "dollar.rvrn"})], featureTag="rvrn"
    )
    return roundtrip(fb.font)


def add_lookup_variation(font):
    gsub = font["GSUB"].table
    feature_indices = {
        record.FeatureTag: index
        for index, record in enumerate(gsub.FeatureList.FeatureRecord)
    }
    condition = ot.ConditionTable()
    condition.Format = 1
    condition.AxisIndex = 0
    condition.FilterRangeMinValue = 0.25
    condition.FilterRangeMaxValue = 1.0

    lookup_indices = ot.LookupIndexList()
    lookup_indices.LookupIndex = list(
        gsub.FeatureList.FeatureRecord[feature_indices["dlig"]].Feature.LookupListIndex
    )
    lookup_indices.LookupIndexCount = len(lookup_indices.LookupIndex)

    lookup_condition = ot.LookupConditionRecord()
    lookup_condition.ConditionTable = condition
    lookup_condition.LookupIndexList = lookup_indices

    feature_lookups = ot.FeatureLookupsTable()
    feature_lookups.Version = 0x00010000
    feature_lookups.Flags = 1
    feature_lookups.LookupConditionRecord = [lookup_condition]
    feature_lookups.LookupConditionCount = 1

    lookup_variation = ot.LookupVariationRecord()
    lookup_variation.FeatureIndex = feature_indices["rvrn"]
    lookup_variation.FeatureLookupsTable = feature_lookups

    gsub.FeatureVariations.Version = 0x00010001
    gsub.FeatureVariations.LookupVariationRecord = [lookup_variation]
    gsub.FeatureVariations.LookupVariationCount = 1


def condition_varstore(lookup_variations, var_idx):
    font = feature_vars_font()
    value = ot.ConditionTable()
    value.Format = 2
    value.DefaultValue = -1
    value.VarIdx = var_idx
    negate = ot.ConditionTable()
    negate.Format = 5
    negate.ConditionTable = value
    condition = ot.ConditionTable()
    condition.Format = 3
    condition.ConditionTable = [None, negate]
    condition.ConditionCount = 2
    variations = font["GSUB"].table.FeatureVariations
    if lookup_variations:
        add_lookup_variation(font)
        variations.LookupVariationRecord[0].FeatureLookupsTable.LookupConditionRecord[
            0
        ].ConditionTable = condition
        variations.FeatureVariationRecord = []
        variations.FeatureVariationCount = 0
    else:
        variations.FeatureVariationRecord[0].ConditionSet.ConditionTable = [condition]

    gdef = ot.GDEF()
    gdef.Version = 0x00010003
    gdef.GlyphClassDef = gdef.AttachList = gdef.LigCaretList = None
    gdef.MarkAttachClassDef = gdef.MarkGlyphSetsDef = None
    gdef.VarStore = buildVarStore(
        buildVarRegionList([{"wght": (0, 1, 1)}], ["wght"]),
        [buildVarData([0], [[99], [2]], optimize=False)],
    )
    font["GDEF"] = newTable("GDEF")
    font["GDEF"].table = gdef
    return font


def feature_dedup():
    font = feature_vars_font()
    add_lookup_variation(font)
    gsub = font["GSUB"].table
    feature_indices = {
        record.FeatureTag: index
        for index, record in enumerate(gsub.FeatureList.FeatureRecord)
    }
    dlig_index = feature_indices["dlig"]
    gsub.FeatureVariations.LookupVariationRecord[0].FeatureIndex = dlig_index
    duplicate = ot.FeatureRecord()
    duplicate.FeatureTag = "dlig"
    duplicate.Feature = gsub.FeatureList.FeatureRecord[dlig_index].Feature
    duplicate_index = len(gsub.FeatureList.FeatureRecord)
    gsub.FeatureList.FeatureRecord.append(duplicate)
    gsub.FeatureList.FeatureCount += 1
    for script_record in gsub.ScriptList.ScriptRecord:
        lang_systems = [script_record.Script.DefaultLangSys]
        lang_systems.extend(r.LangSys for r in script_record.Script.LangSysRecord)
        for lang_system in filter(None, lang_systems):
            if dlig_index in lang_system.FeatureIndex:
                lang_system.FeatureIndex.append(duplicate_index)
                lang_system.FeatureCount += 1
    return font


def null_substitution(explicit_empty):
    font = feature_vars_font()
    variations = font["GSUB"].table.FeatureVariations
    first = deepcopy(variations.FeatureVariationRecord[0])
    first.ConditionSet.ConditionTable[0].FilterRangeMinValue = 0.5
    if explicit_empty:
        first.FeatureTableSubstitution.SubstitutionRecord = []
        first.FeatureTableSubstitution.SubstitutionCount = 0
    else:
        first.FeatureTableSubstitution = None
    last = deepcopy(first)
    last.ConditionSet.ConditionTable[0].FilterRangeMinValue = -1
    last.ConditionSet.ConditionTable[0].FilterRangeMaxValue = 0
    variations.FeatureVariationRecord.insert(0, first)
    variations.FeatureVariationRecord.append(last)
    variations.FeatureVariationCount = 3
    return font


def singlepos2_font():
    fb = FontBuilder(unitsPerEm=1000)
    fb.setupGlyphOrder([".notdef", "a", "b", "c"])
    fb.setupCharacterMap({ord("a"): "a", ord("b"): "b", ord("c"): "c"})
    fb.setupNameTable({"familyName": "TestSingePosFormat", "styleName": "Regular"})
    fb.setupPost()
    fb.addOpenTypeFeatures("""
        feature kern {
            pos a -50;
            pos b -40;
            pos c -50;
        } kern;
    """)
    return roundtrip(fb.font)


def null_gdef_varstore(keep_classes):
    font = singlepos2_font()
    gdef = ot.GDEF()
    gdef.Version = 0x00010003
    gdef.AttachList = gdef.LigCaretList = None
    gdef.MarkAttachClassDef = gdef.MarkGlyphSetsDef = None
    gdef.VarStore = None
    gdef.GlyphClassDef = ot.ClassDef()
    gdef.GlyphClassDef.classDefs = {"a" if keep_classes else "b": 1}
    font["GDEF"] = newTable("GDEF")
    font["GDEF"].table = gdef
    return font


def variable_notdef():
    fb = FontBuilder(1000)
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


def composite_closure(nested, varc):
    order = [".notdef", "leaf", "unused", "composite"]
    glyphs = {name: TTGlyphPen(None).glyph() for name in order}
    pen = TTGlyphPen(None)
    pen.moveTo((100, 0))
    pen.lineTo((200, 0))
    pen.lineTo((100, 100))
    pen.closePath()
    glyphs["leaf"] = pen.glyph()
    if nested:
        pen = TTGlyphPen(glyphs)
        pen.addComponent("leaf", (1, 0, 0, 1, 100, 50))
        glyphs["unused"] = pen.glyph()
    pen = TTGlyphPen(glyphs)
    pen.addComponent("unused" if nested else "leaf", (1, 0, 0, 1, 100, 50))
    glyphs["composite"] = pen.glyph()

    fb = FontBuilder(1000)
    fb.setupGlyphOrder(order)
    fb.setupCharacterMap({65: "composite"})
    fb.setupGlyf(glyphs)
    fb.setupHorizontalMetrics({name: (500, 100) for name in order})
    fb.setupHorizontalHeader()
    fb.setupNameTable({"familyName": "Composite closure", "styleName": "Regular"})
    fb.setupOS2()
    fb.setupPost()
    if varc:
        component = ot.VarComponent()
        component.glyphName = "composite"
        glyph = ot.VarCompositeGlyph()
        glyph.components = [component]
        table = ot.VARC()
        table.Version = 0x10000
        table.Coverage = ot.Coverage()
        table.Coverage.glyphs = ["composite"]
        table.MultiVarStore = table.AxisIndicesList = table.ConditionList = None
        table.VarCompositeGlyphs = ot.VarCompositeGlyphs()
        table.VarCompositeGlyphs.VarCompositeGlyph = [glyph]
        fb.font["VARC"] = newTable("VARC")
        fb.font["VARC"].table = table
    return fb.font


def colrv1_font():
    base_glyph_names = ["uni%04X" % i for i in range(0xE000, 0xE000 + 10)]
    layer_glyph_names = ["glyph%05d" % i for i in range(10, 20)]
    glyph_order = [".notdef"] + base_glyph_names + layer_glyph_names
    pen = TTGlyphPen(glyphSet=None)
    pen.moveTo((0, 0))
    pen.lineTo((0, 500))
    pen.lineTo((500, 500))
    pen.lineTo((500, 0))
    pen.closePath()
    glyph = pen.glyph()
    fb = FontBuilder(unitsPerEm=1024, isTTF=True)
    fb.setupGlyphOrder(glyph_order)
    fb.setupCharacterMap({int(name[3:], 16): name for name in base_glyph_names})
    fb.setupGlyf({g: glyph for g in glyph_order})
    fb.setupHorizontalMetrics({g: (500, 0) for g in glyph_order})
    fb.setupHorizontalHeader()
    fb.setupOS2()
    fb.setupPost()
    fb.setupNameTable({"familyName": "TestCOLRv1", "styleName": "Regular"})
    fb.setupCOLR(
        {
            "uniE000": (
                ot.PaintFormat.PaintColrLayers,
                [
                    {
                        "Format": ot.PaintFormat.PaintGlyph,
                        "Paint": (ot.PaintFormat.PaintSolid, 0),
                        "Glyph": "glyph00010",
                    },
                    {
                        "Format": ot.PaintFormat.PaintGlyph,
                        "Paint": (ot.PaintFormat.PaintSolid, 2, 0.3),
                        "Glyph": "glyph00011",
                    },
                ],
            ),
            "uniE001": (
                ot.PaintFormat.PaintColrLayers,
                [
                    {
                        "Format": ot.PaintFormat.PaintTransform,
                        "Paint": {
                            "Format": ot.PaintFormat.PaintGlyph,
                            "Paint": {
                                "Format": ot.PaintFormat.PaintRadialGradient,
                                "x0": 250,
                                "y0": 250,
                                "r0": 250,
                                "x1": 200,
                                "y1": 200,
                                "r1": 0,
                                "ColorLine": {
                                    "ColorStop": [(0.0, 1), (1.0, 2)],
                                    "Extend": "repeat",
                                },
                            },
                            "Glyph": "glyph00012",
                        },
                        "Transform": (0.7071, 0.7071, -0.7071, 0.7071, 0, 0),
                    },
                    {
                        "Format": ot.PaintFormat.PaintGlyph,
                        "Paint": (ot.PaintFormat.PaintSolid, 1, 0.5),
                        "Glyph": "glyph00013",
                    },
                ],
            ),
            "uniE002": (
                ot.PaintFormat.PaintColrLayers,
                [
                    {
                        "Format": ot.PaintFormat.PaintGlyph,
                        "Paint": {
                            "Format": ot.PaintFormat.PaintLinearGradient,
                            "x0": 0,
                            "y0": 0,
                            "x1": 500,
                            "y1": 500,
                            "x2": -500,
                            "y2": 500,
                            "ColorLine": {"ColorStop": [(0.0, 1), (1.0, 2)]},
                        },
                        "Glyph": "glyph00014",
                    },
                    {
                        "Format": ot.PaintFormat.PaintTransform,
                        "Paint": {
                            "Format": ot.PaintFormat.PaintGlyph,
                            "Paint": (ot.PaintFormat.PaintSolid, 1),
                            "Glyph": "glyph00015",
                        },
                        "Transform": (1, 0, 0, 1, 400, 400),
                    },
                ],
            ),
            "uniE003": {
                "Format": ot.PaintFormat.PaintRotateAroundCenter,
                "Paint": {
                    "Format": ot.PaintFormat.PaintColrGlyph,
                    "Glyph": "uniE001",
                },
                "angle": 45,
                "centerX": 250,
                "centerY": 250,
            },
            "uniE004": [
                ("glyph00016", 1),
                ("glyph00017", 0xFFFF),  # special palette index for foreground text
                ("glyph00018", 2),
            ],
        },
        clipBoxes={
            "uniE000": (0, 0, 200, 300),
            "uniE001": (0, 0, 500, 500),
            "uniE002": (-50, -50, 400, 400),
            "uniE003": (-50, -50, 400, 400),
        },
    )
    fb.setupCPAL(
        [
            [
                (1.0, 0.0, 0.0, 1.0),  # red
                (0.0, 1.0, 0.0, 1.0),  # green
                (0.0, 0.0, 1.0, 1.0),  # blue
            ],
        ],
    )
    return fb.font


def colrv1_cpalv1(share_name_id):
    font = roundtrip(colrv1_font())
    fb = FontBuilder(font=font)
    fb.setupCPAL(
        [
            [
                (1.0, 0.0, 0.0, 1.0),  # red
                (0.0, 1.0, 0.0, 1.0),  # green
                (0.0, 0.0, 1.0, 1.0),  # blue
            ],
        ],
        paletteLabels=["test palette"],
        paletteEntryLabels=["first color", "second color", "third color"],
    )
    if share_name_id:
        # The name ID of the first color is 1, the family name "TestCOLRv1".
        fb.font["CPAL"].paletteEntryLabels[0] = 1
    return fb.font


def keep_size_font():
    fb = FontBuilder(unitsPerEm=1000, isTTF=True)
    glyph_order = [".notdef", "a", "b", "b.ss01"]
    fb.setupGlyphOrder(glyph_order)
    fb.setupGlyf({g: TTGlyphPen(None).glyph() for g in glyph_order})
    fb.setupCharacterMap({ord("a"): "a", ord("b"): "b"})
    fb.setupHorizontalMetrics({g: (500, 0) for g in glyph_order})
    fb.setupHorizontalHeader()
    fb.setupOS2()
    fb.setupPost()
    fb.setupNameTable({"familyName": "TestKeepSizeFeature", "styleName": "Regular"})
    fb.addOpenTypeFeatures("""
        feature size {
          parameters 10.0 0;
        } size;
        feature ss01 {
          featureNames {
            name "Alternate b";
          };
          sub b by b.ss01;
        } ss01;
    """)
    return fb.font


def mark_glyph_sets_font():
    fb = FontBuilder(unitsPerEm=1000, isTTF=True)
    glyph_order = [
        ".notdef", "A", "Aacute", "Acircumflex", "Adieresis", "a", "aacute",
        "acircumflex", "adieresis", "dieresiscomb", "acutecomb", "circumflexcomb",
    ]
    fb.setupGlyphOrder(glyph_order)
    fb.setupGlyf({g: TTGlyphPen(None).glyph() for g in glyph_order})
    fb.setupHorizontalMetrics({g: (500, 0) for g in glyph_order})
    fb.setupHorizontalHeader()
    fb.setupPost()
    fb.setupNameTable(
        {"familyName": "TestGDEFMarkGlyphSetsDef", "styleName": "Regular"}
    )
    fb.addOpenTypeFeatures("""
        feature ccmp {
            lookup ccmp_1 {
                lookupflag UseMarkFilteringSet [acutecomb];
                sub a acutecomb by aacute;
                sub A acutecomb by Aacute;
            } ccmp_1;
            lookup ccmp_2 {
                lookupflag UseMarkFilteringSet [circumflexcomb];
                sub a circumflexcomb by acircumflex;
                sub A circumflexcomb by Acircumflex;
            } ccmp_2;
            lookup ccmp_3 {
                lookupflag UseMarkFilteringSet [dieresiscomb];
                sub a dieresiscomb by adieresis;
                sub A dieresiscomb by Adieresis;
                sub A acutecomb by Aacute;
            } ccmp_3;
        } ccmp;
    """)
    return fb.font


def empty_svg_font():
    """`empty_svg_font` of Tests/subset/svg_test.py, without the empty table."""
    glyph_order = [".notdef"] + list(ascii_letters)
    pen = TTGlyphPen(glyphSet=None)
    pen.moveTo((0, 0))
    pen.lineTo((0, 500))
    pen.lineTo((500, 500))
    pen.lineTo((500, 0))
    pen.closePath()
    glyph = pen.glyph()
    fb = FontBuilder(unitsPerEm=1024, isTTF=True)
    fb.setupGlyphOrder(glyph_order)
    fb.setupCharacterMap({ord(c): c for c in ascii_letters})
    fb.setupGlyf({g: glyph for g in glyph_order})
    fb.setupHorizontalMetrics({g: (500, 0) for g in glyph_order})
    fb.setupHorizontalHeader()
    fb.setupOS2()
    fb.setupPost()
    fb.setupNameTable({"familyName": "TestSVG", "styleName": "Regular"})
    return fb.font


def max_context_font():
    """`test_max_ctx_calc_features` of Tests/otlLib/maxContextCalc_test.py."""
    from fontTools.feaLib.builder import addOpenTypeFeaturesFromString

    font = TTFont()
    font.setGlyphOrder(".notdef space A B C a b c".split())
    addOpenTypeFeaturesFromString(
        font,
        """
    lookup GSUB_EXT useExtension {
        sub a by b;
    } GSUB_EXT;

    lookup GPOS_EXT useExtension {
        pos a b -10;
    } GPOS_EXT;

    feature sub1 {
        sub A by a;
        sub A B by b;
        sub A B C by c;
        sub [A B] C by c;
        sub [A B] C [A B] by c;
        sub A' C by A B;
        sub a' by b;
        sub a' b by c;
        sub a from [A B C];
        rsub a by b;
        rsub a' by b;
        rsub a b' by c;
        rsub a b' c by A;
        rsub [a b] [a b c]' [a b] by B;
        rsub [a b] c' by A;
        lookup GSUB_EXT;
    } sub1;

    feature pos1 {
        pos A 20;
        pos A B -50;
        pos A B' 10 C;
        lookup GPOS_EXT;
    } pos1;
    """,
    )
    # For reading the compiled tables back, which need the glyph count.
    font["maxp"] = newTable("maxp")
    font["maxp"].tableVersion = 0x00005000
    font["maxp"].numGlyphs = len(font.getGlyphOrder())
    OTLLIB.mkdir(parents=True, exist_ok=True)
    with open(OTLLIB / "max_context.ttx", "w", encoding="utf-8", newline="\n") as f:
        font.saveXML(f, newlinestr="\n")
    print("max_context")


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    max_context_font()
    write(empty_svg_font(), "empty_svg")
    write(feature_vars_font(), "feature_vars")
    font = feature_vars_font()
    add_lookup_variation(font)
    write(font, "lookup_variation")
    for lookup_variations in (False, True):
        for var_idx in (1, 7, 0x10000):
            write(
                condition_varstore(lookup_variations, var_idx),
                f"condition_varstore_{int(lookup_variations)}_{var_idx}",
            )
    write(feature_dedup(), "feature_dedup")
    for explicit_empty in (False, True):
        write(null_substitution(explicit_empty), f"null_substitution_{int(explicit_empty)}")
    write(singlepos2_font(), "singlepos2")
    for keep_classes in (False, True):
        write(null_gdef_varstore(keep_classes), f"null_gdef_varstore_{int(keep_classes)}")
    write(variable_notdef(), "variable_notdef")
    for nested in (False, True):
        for varc in (False, True):
            write(composite_closure(nested, varc), f"composite_closure_{int(nested)}_{int(varc)}")
    write(colrv1_font(), "colrv1")
    write(colrv1_cpalv1(False), "colrv1_cpalv1")
    write(colrv1_cpalv1(True), "colrv1_cpalv1_share_name_id")
    write(keep_size_font(), "keep_size")
    write(mark_glyph_sets_font(), "mark_glyph_sets")


if __name__ == "__main__":
    main()
