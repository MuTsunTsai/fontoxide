"""Generates the fixtures of crates/fontoxide-varlib/tests/data/mutator.

Two tests of fontTools' Tests/varLib/mutator_test.py build their fonts with
FontBuilder and featureVars, which are not ported yet. This script builds the same
fonts and saves them as TTX.

    uv run --project tools/reference python tools/reference/mutator_fixtures.py
"""

import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "upstream" / "fonttools" / "Lib"))
sys.dont_write_bytecode = True

from fontTools.fontBuilder import FontBuilder
from fontTools.varLib.featureVars import (
	addFeatureVariations,
	buildConditionTable,
	buildFeatureTableSubstitutionRecord,
	buildFeatureVariationRecord,
)

OUT = ROOT / "crates" / "fontoxide-varlib" / "tests" / "data" / "mutator"


def base_font(conditions):
	fb = FontBuilder(1000)
	fb.setupGlyphOrder([".notdef", "A", "A.alt"])
	fb.setupPost()
	fb.setupNameTable({"familyName": "Test", "styleName": "Regular"})
	fb.setupFvar([("wght", 100, 400, 900, "Weight")], [])
	font = fb.font
	addFeatureVariations(font, [(conditions, {"A": "A.alt"})], featureTag="ccmp")
	return font


def null_condition_set():
	"""`test_mutator_null_condition_set`."""
	font = base_font([{"wght": (-1.0, 1.0)}])
	record = font["GSUB"].table.FeatureVariations.FeatureVariationRecord[0]
	assert record.ConditionSet is None
	return font


def null_substitution_first_match():
	"""`test_mutator_null_substitution_first_match`."""
	font = base_font([{"wght": (0.5, 1.0)}])
	gsub = font["GSUB"].table
	record = gsub.FeatureVariations.FeatureVariationRecord[0]
	record.FeatureTableSubstitution = None
	# A matching null substitution blocks the later matching record.
	later = buildFeatureVariationRecord(
		[buildConditionTable(0, 0.5, 1.0)],
		[buildFeatureTableSubstitutionRecord(0, [0])],
	)
	gsub.FeatureVariations.FeatureVariationRecord.append(later)
	gsub.FeatureVariations.FeatureVariationCount += 1
	return font


def main():
	OUT.mkdir(parents=True, exist_ok=True)
	for name, build in [
		("null_condition_set", null_condition_set),
		("null_substitution_first_match", null_substitution_first_match),
	]:
		font = build()
		font.saveXML(OUT / f"{name}.ttx", newlinestr="\n")
		print(name)


if __name__ == "__main__":
	main()
