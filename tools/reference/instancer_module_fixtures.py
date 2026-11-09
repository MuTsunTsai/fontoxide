"""Generates the fixtures of crates/fontoxide-varlib/tests/data/instancer_modules.

fontTools' Tests/varLib/instancer/{cff2,cff2_private,jstf,math,varc}_test.py build
their test fonts with FontBuilder, feaLib and varLib, which are not ported yet. This
script runs the same code, by importing the helpers of those files, and saves the
resulting fonts as TTX. Where a test changes a font in a way that the Rust side can't
easily express (charstring programs, Private DICTs), its changes are made here too,
copied from the test.

    uv run --project tools/reference python tools/reference/instancer_module_fixtures.py
"""

import importlib.util
import io
import pathlib
import sys
from types import SimpleNamespace

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "upstream" / "fonttools" / "Lib"))
sys.dont_write_bytecode = True

from fontTools.ttLib import TTFont
from fontTools.ttLib.tables import otTables
from fontTools.varLib import builder

OUT = ROOT / "crates" / "fontoxide-varlib" / "tests" / "data" / "instancer_modules"
TESTS = ROOT / "upstream" / "fonttools" / "Tests" / "varLib" / "instancer"


def load(name):
    spec = importlib.util.spec_from_file_location(name, TESTS / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def unwrap(fixture):
    """The function under a `pytest.fixture`."""
    if hasattr(fixture, "_get_wrapped_function"):
        return fixture._get_wrapped_function()
    return fixture.__wrapped__


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


def cff2_fixtures():
    t = load("cff2_test")
    write(t.makeRingVF(t.TWO_MASTERS), "cff2_two")
    write(t.makeRingVF(t.SIX_MASTERS), "cff2_six")
    write(t.makeRingVF(t.OFF_END_MASTERS), "cff2_off_end")
    write(t.makeRingVF(t.TWO_MASTERS, hinted=True), "cff2_two_hinted")
    write(t.makeRingVF(t.TWO_MASTERS, flex=True), "cff2_two_flex")

    # test_private_dict_vsindex
    vf = t.makeRingVF(t.TWO_MASTERS)
    topDict = vf["CFF2"].cff.topDictIndex[0]
    varStore = topDict.VarStore.otVarStore
    varStore.VarData.insert(0, builder.buildVarData([], []))
    varStore.VarDataCount = len(varStore.VarData)
    topDict.FDArray[0].Private.vsindex = 1
    write(vf, "cff2_private_dict_vsindex")

    # test_ghost_hint
    write(
        t.makeVF(
            [
                ("hstemhm", [[101, 3, 1], -21]),
                ("hintmask", []),
                ("", [b"\x80"]),
                ("rmoveto", [[100, 3, 1], 100]),
                ("rlineto", [51, 0, 0, 50]),
            ]
        ),
        "cff2_ghost_hint",
    )
    # test_rounding_keeps_distances
    write(
        t.makeVF([("rmoveto", [[100, 3, 1], 100]), ("rlineto", [51, 0, 0, 50])]),
        "cff2_rounding_keeps_distances",
    )
    # test_fractional_outline
    write(
        t.makeVF(
            [("rmoveto", [[100.25, 3, 1], 100.5]), ("rlineto", [51, 0, 0, 50.5])]
        ),
        "cff2_fractional_outline",
    )
    # test_flex1_changing_direction and test_flex1_direction_at_pinned_location
    flex1 = t.InstantiateCFF2RoundingTest.FLEX1_CHANGING_DIRECTION
    commands = [("rmoveto", [100, 100]), ("flex1", flex1), ("rlineto", [-50, 0])]
    write(t.makeVF(commands), "cff2_flex1")
    # test_flex1_direction_with_invalid_tent
    vf = t.makeVF(commands)
    varStore = vf["CFF2"].cff.topDictIndex[0].VarStore.otVarStore
    varStore.VarRegionList.Region[0].VarRegionAxis[0].EndCoord = 0.5
    write(vf, "cff2_flex1_invalid_tent")
    # test_vsindex_after_blend
    write(
        t.makeVF(
            [
                ("rmoveto", [100, [100, 50, 1]]),
                ("vsindex", [0]),
                ("rlineto", [[50, 10, 1], 0, 0, 50, -50, 0]),
            ]
        ),
        "cff2_vsindex_after_blend",
    )
    # test_fractional_private_dict_values
    vf = t.makeRingVF(t.TWO_MASTERS)
    private = vf["CFF2"].cff.topDictIndex[0].FDArray[0].Private
    private.BlueScale = [0.039625, 0.02]
    write(vf, "cff2_fractional_private")
    # test_private_dict_vsindex_and_charstring_vsindex
    vf = t.makeRingVF(t.TWO_MASTERS)
    topDict = vf["CFF2"].cff.topDictIndex[0]
    varStore = topDict.VarStore.otVarStore
    region = builder.buildVarRegion({"slnt": (-1, -1, 0)}, ["wght", "slnt"])
    varStore.VarRegionList.Region.append(region)
    varStore.VarRegionList.RegionCount += 1
    varStore.VarData.append(
        builder.buildVarData([len(varStore.VarRegionList.Region) - 1], [])
    )
    varStore.VarDataCount = len(varStore.VarData)
    topDict.FDArray[0].Private.vsindex = 1
    for charString in topDict.CharStrings.values():
        charString.decompile()
        charString.program = [0, "vsindex"] + charString.program
    write(vf, "cff2_private_and_charstring_vsindex")


def cff2_private_fixtures():
    t = load("cff2_private_test")
    write(t.makeFont(), "cff2_private")
    # test_fractional_private_blends_inherited_vsindex
    original = t.makeFont()
    topDict = original["CFF2"].cff.topDictIndex[0]
    private = topDict.FDArray[0].Private
    store = topDict.VarStore.otVarStore
    store.VarData.insert(0, builder.buildVarData([], []))
    store.VarDataCount += 1
    private.vsindex = 1
    write(original, "cff2_private_inherited_vsindex")


def varc_fixtures():
    t = load("varc_test")
    varc_font = unwrap(t.varc_font)
    write(varc_font(SimpleNamespace(param=False)), "varc_no_avar")
    write(varc_font(SimpleNamespace(param=True)), "varc_avar2")
    # test_avar2_preserves_component_internal_variation
    font = varc_font(SimpleNamespace(param=True))
    axes = font["fvar"].axes
    axis_tags = [axis.axisTag for axis in axes]
    region_list = builder.buildVarRegionList([{"DUMY": (0, 1, 1)}], axis_tags)
    var_data = builder.buildVarData([0], [[16384]], optimize=False)
    avar = font["avar"]
    avar.table.VarStore = builder.buildVarStore(region_list, [var_data])
    mapping = [otTables.NO_VARIATION_INDEX] * len(axes)
    mapping[axis_tags.index("0000")] = 0
    avar.table.VarIdxMap = builder.buildDeltaSetIndexMap(mapping)
    write(font, "varc_avar2_driven")


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    cff2_fixtures()
    cff2_private_fixtures()
    write(unwrap(load("jstf_test").font)(), "jstf")
    write(unwrap(load("math_test").font)(), "math")
    varc_fixtures()


if __name__ == "__main__":
    main()
