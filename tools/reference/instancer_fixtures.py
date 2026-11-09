"""Generates the fixtures of crates/fontoxide-varlib/tests/data/instancer.

fontTools' Tests/varLib/instancer/instancer_test.py builds some of its test fonts
with feaLib, FontBuilder and varLib, which are not ported yet. This script runs the
same code as the helpers of that file, by importing them, and saves the resulting
fonts as TTX. What the tests then change in those fonts is done in the Rust tests.

    uv run --project tools/reference python tools/reference/instancer_fixtures.py
"""

import importlib.util
import io
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "upstream" / "fonttools" / "Lib"))
sys.dont_write_bytecode = True

from fontTools.feaLib.builder import addOpenTypeFeaturesFromString
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools import ttLib
from fontTools.ttLib import TTFont, newTable
from fontTools.ttLib.tables import otTables
from fontTools.varLib import builder, featureVars

OUT = ROOT / "crates" / "fontoxide-varlib" / "tests" / "data" / "instancer"
TEST_FILE = (
    ROOT / "upstream" / "fonttools" / "Tests" / "varLib" / "instancer" / "instancer_test.py"
)

spec = importlib.util.spec_from_file_location("instancer_test", TEST_FILE)
upstream = importlib.util.module_from_spec(spec)
spec.loader.exec_module(upstream)


def unwrap(fixture):
    """The function of a pytest fixture."""
    return getattr(fixture, "__wrapped__", None) or fixture.__pytest_wrapped__.obj


def roundtrip(font):
    buf = io.BytesIO()
    font.save(buf)
    buf.seek(0)
    return TTFont(buf, recalcTimestamp=False)


def write(font, name):
    """Saves a font as TTX, from memory: the test fonts may not be complete."""
    with open(OUT / f"{name}.ttx", "w", encoding="utf-8", newline="\n") as f:
        font.saveXML(f, newlinestr="\n")
    print(name)


GPOS_FEATURES = """
    lookup Default { pos A 10; } Default;
    lookup Conditional { pos B 20; } Conditional;
    feature kern { lookup Default; } kern;
    feature zzzz { lookup Conditional; } zzzz;
"""

THREE_RULES = [
    ([{"wght": (0.20886, 1.0)}], {"uni0024": "uni0024.nostroke"}),
    ([{"cntr": (0.75, 1.0)}], {"uni0041": "uni0061"}),
    ([{"wght": (-1.0, -0.45654), "cntr": (0, 0.25)}], {"uni0061": "uni0041"}),
]


def null_substitution_font():
    font = upstream.makeFeatureVarsFont([([{"wght": (0.25, 1.0)}], {"A": "A.alt"})])
    gsub = font["GSUB"].table
    featureVars.buildSubstitutionLookups(gsub, [(("A.alt", "A"),)], processLast=True)
    feature = gsub.FeatureList.FeatureRecord[0].Feature
    feature.LookupListIndex = [1]
    feature.LookupCount = 1
    return font


def rules_font():
    fb = FontBuilder(1000)
    order = [".notdef", "dollar", "dollar.nostroke", "A", "a"]
    fb.setupGlyphOrder(order)
    fb.setupCharacterMap({0x24: "dollar", 0x41: "A", 0x61: "a"})
    fb.setupGlyf({name: TTGlyphPen(None).glyph() for name in order})
    fb.setupHorizontalMetrics({name: (500, 0) for name in order})
    fb.setupHorizontalHeader()
    fb.setupNameTable({"familyName": "Rules", "styleName": "Regular"})
    fb.setupOS2()
    fb.setupPost()
    fb.setupFvar(
        [
            ("wght", 100, 400, 900, "Weight"),
            ("wdth", 50, 100, 200, "Width"),
        ],
        [],
    )
    featureVars.addFeatureVariations(
        fb.font,
        [
            ([{"wght": (0.2, 1)}], {"dollar": "dollar.nostroke"}),
            ([{"wdth": (0.75, 1)}], {"A": "a"}),
        ],
    )
    fb.font["head"].created = fb.font["head"].modified = 0
    return roundtrip(fb.font)


def conditions_font():
    fb = FontBuilder(1000)
    order = [".notdef", "dollar", "dollar.nostroke"]
    fb.setupGlyphOrder(order)
    fb.setupCharacterMap({0x24: "dollar"})
    fb.setupGlyf({name: TTGlyphPen(None).glyph() for name in order})
    fb.setupHorizontalMetrics({name: (500, 0) for name in order})
    fb.setupHorizontalHeader()
    fb.setupNameTable({"familyName": "Conditions", "styleName": "Regular"})
    fb.setupOS2()
    fb.setupPost()
    fb.setupFvar(
        [("wght", 100, 400, 900, "Weight"), ("wdth", 50, 100, 200, "Width")],
        [],
    )
    font = fb.font
    featureVars.addFeatureVariations(
        font, [([{"wght": (0.5, 1.0)}], {"dollar": "dollar.nostroke"})]
    )
    font["head"].created = font["head"].modified = 0
    return font


def other_gpos_font():
    font = upstream.makeLookupVariationsFont()
    other = ttLib.TTFont()
    other.setGlyphOrder(font.getGlyphOrder())
    addOpenTypeFeaturesFromString(other, "feature kern { pos B 10; } kern;")
    return other


def vvar_font(version):
    """The `varfont` fixture of InstantiateVVARTest."""
    fvarAxes = unwrap(upstream.fvarAxes)()
    font = unwrap(upstream.InstantiateVVARTest.varfont)(
        None, fvarAxes, type("Request", (), {"param": version})()
    )
    # Only set when decompiling, while saveXML needs it.
    font["VORG"].numVertOriginYMetrics = len(font["VORG"].VOriginRecords)
    return font


def main():
    OUT.mkdir(parents=True, exist_ok=True)

    write(unwrap(upstream.varfontGDEF)(), "otl_gdef")
    write(unwrap(upstream.varfontGPOS)(), "otl_gpos")
    write(unwrap(upstream.varfontGPOS2)(), "otl_gpos2")
    write(unwrap(upstream.varfontBASE)(), "otl_base")

    write(upstream.makeLookupVariationsFont(), "lookup_variations")
    write(upstream.makeLookupVariationsFont(withVarStore=True), "lookup_variations_store")
    font = upstream.makeLookupVariationsFont(withVarStore=True)
    addOpenTypeFeaturesFromString(font, GPOS_FEATURES, tables=["GPOS"])
    write(font, "lookup_variations_store_gpos")
    for format in (2, 3, 4, 5):
        write(upstream.makeFeatureVariationConditionsFont(format), f"conditions_{format}")

    write(upstream.makeFeatureVarsFont(THREE_RULES), "feature_vars_three_rules")
    write(
        upstream.makeFeatureVarsFont(
            [([{"wght": (-1.0, 1.0)}], {"uni0024": "uni0024.nostroke"})]
        ),
        "feature_vars_null_conditionset",
    )
    write(null_substitution_font(), "feature_vars_null_substitution")
    write(rules_font(), "feature_vars_rules")
    write(conditions_font(), "feature_vars_unsupported_condition")
    write(other_gpos_font(), "other_gpos")

    write(vvar_font(1), "vvar_1")
    write(vvar_font(2), "vvar_2")


main()
