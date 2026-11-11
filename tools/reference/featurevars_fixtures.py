"""Generates the fixtures of crates/fontoxide-varlib/tests/data/feature_vars.

fontTools' Tests/varLib/featureVars_test.py builds its fonts with varLib._add_fvar
and feaLib, which are not available to the tests of fontoxide-varlib. This script
runs the same code and saves:

- varfont.ttx: the `varfont` fixture (`makeVariableFont`), without its OS/2
  table, which only has `usMaxContext` and cannot be saved as TTX; the tests add
  one.
- extended_<existing>.ttx: the font of `test_addFeatureVariations_extended_layout`
  before the parametrized conversions, and extended_<existing>_<tag>.expected.ttx
  the GSUB of its reference after `addFeatureVariations` and a roundtrip.
- raw_<tag>.ttx and raw_<tag>.expected.ttx: the same for
  `test_addFeatureVariationsRaw_extended_layout`.

    uv run --project tools/reference python tools/reference/featurevars_fixtures.py
"""

import pathlib
import sys
from collections import OrderedDict
from copy import deepcopy

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "upstream" / "fonttools" / "Lib"))
sys.dont_write_bytecode = True

from fontTools import varLib
from fontTools.designspaceLib import AxisDescriptor
from fontTools.feaLib.builder import addOpenTypeFeaturesFromString
from fontTools.otlLib import builder
from fontTools.ttLib import TTFont, newTable
from fontTools.varLib.featureVars import addFeatureVariations, addFeatureVariationsRaw

OUT = ROOT / "crates" / "fontoxide-varlib" / "tests" / "data" / "feature_vars"


def makeVariableFont(glyphOrder, axes):
	"""`makeVariableFont` of the test, without OS/2."""
	font = TTFont()
	font.setGlyphOrder(glyphOrder)
	font["name"] = newTable("name")
	ds_axes = OrderedDict()
	for axisTag, (minimum, default, maximum) in axes.items():
		axis = AxisDescriptor()
		axis.name = axis.tag = axis.labelNames["en"] = axisTag
		axis.minimum, axis.default, axis.maximum = minimum, default, maximum
		ds_axes[axisTag] = axis
	varLib._add_fvar(font, ds_axes, instances=())
	return font


def varfont():
	return makeVariableFont(
		[".notdef", "space", "A", "B", "A.alt", "B.alt"],
		{"wght": (100, 400, 900)},
	)


def roundtrip(font, tag):
	data = font[tag].compile(font)
	table = newTable(tag)
	table.decompile(data, font)
	font[tag] = table


def save(font, name, tables=None):
	with open(OUT / name, "w", encoding="utf-8", newline="\n") as f:
		font.saveXML(f, tables=tables, newlinestr="\n")
	print(name)


def extended_layout(existing_variation):
	"""The font of `test_addFeatureVariations_extended_layout` before conversions."""
	font = varfont()
	addOpenTypeFeaturesFromString(font, "feature calt { sub B A' by A.alt; } calt;")
	lookup_builder = builder.ChainContextSubstBuilder(font, None)
	referenced_lookup = builder.SingleSubstBuilder(font, None)
	referenced_lookup.lookup_index = 1
	rule = builder.ChainContextualRule(
		[], [["A", "B"], ["A", "B"]], [], [[referenced_lookup], None]
	)
	ruleset = builder.ChainContextualRuleset()
	ruleset.addRule(rule)
	font["GSUB"].table.LookupList.Lookup[0] = builder.buildLookup(
		[lookup_builder.buildFormat1Subtable(ruleset, False)]
	)
	if existing_variation:
		addFeatureVariations(
			font, [([{"wght": (-1.0, 0.0)}], {"B": "B.alt"})], featureTag="ccmp"
		)
	return font


def raw_layout(tag):
	"""The font of `test_addFeatureVariationsRaw_extended_layout`."""
	font = varfont()
	features = (
		"feature calt { sub A by A.alt; } calt;"
		if tag == "GSUB"
		else "feature kern { pos A -20; } kern;"
	)
	addOpenTypeFeaturesFromString(font, features)
	return font


def main():
	OUT.mkdir(parents=True, exist_ok=True)
	save(varfont(), "varfont.ttx")

	for existing in (False, True):
		name = "existing" if existing else "plain"
		font = extended_layout(existing)
		save(font, f"extended_{name}.ttx")
		for feature_tag in ("rvrn", "rclt"):
			reference = deepcopy(font)
			substitutions = [([{"wght": (0.5, 1.0)}], {"A": "A.alt"})]
			addFeatureVariations(reference, substitutions, featureTag=feature_tag)
			roundtrip(reference, "GSUB")
			save(reference, f"extended_{name}_{feature_tag}.expected.ttx", ["GSUB"])

	for tag in ("GSUB", "GPOS"):
		font = raw_layout(tag)
		save(font, f"raw_{tag}.ttx")
		reference = deepcopy(font)
		addFeatureVariationsRaw(
			reference, reference[tag].table, [({"wght": (0.5, 1.0)}, [0])]
		)
		roundtrip(reference, tag)
		save(reference, f"raw_{tag}.expected.ttx", [tag])


if __name__ == "__main__":
	main()
