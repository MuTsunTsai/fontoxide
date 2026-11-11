"""Generates the fixtures of crates/fontoxide-ufolib/tests/data.

`GlyphSetTests.testRoundTrip` of Tests/ufoLib/glifLib_test.py reads the glyphs
of DemoFont.ufo and writes them back as GLIF 1 (a UFO 2 glyph set), then
compares the files with difflib. This script saves what fontTools writes, so
that the Rust test can compare the files byte by byte:

- roundtrip/<file name>.glif: the glyphs written by `GlyphSet.writeGlyph`.

It also converts the test fonts of Tests/ufoLib/testdata between UFO versions,
reading everything with `UFOReader` and writing it with `UFOWriter`, so that
the Rust reader and writer can be compared with fontTools file by file (a test
that is not in fontTools):

- convert/<font> to <version>.ufo: TestFont1 (UFO1, UFO2 and UFO3) written as
  UFO 1, 2 and 3 (not to a lower version than the source).

    uv run --project tools/reference python tools/reference/ufolib_fixtures.py
"""

import pathlib
import shutil
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "upstream" / "fonttools" / "Lib"))
sys.dont_write_bytecode = True

from fontTools.ufoLib import UFOReader, UFOWriter  # noqa: E402
from fontTools.ufoLib.glifLib import GlyphSet  # noqa: E402

TESTDATA = ROOT / "upstream" / "fonttools" / "Tests" / "ufoLib" / "testdata"
DEMO = TESTDATA / "DemoFont.ufo" / "glyphs"
OUT = ROOT / "crates" / "fontoxide-ufolib" / "tests" / "data"


class Info:
	pass


def convert_font(version, target_version, dest):
	"""Copies everything of a test font to a new UFO."""
	reader = UFOReader(str(TESTDATA / f"TestFont1 (UFO{version}).ufo"))
	writer = UFOWriter(str(dest), formatVersion=target_version)
	writer.setKerningGroupConversionRenameMaps(reader.getKerningGroupConversionRenameMaps())
	info = Info()
	reader.readInfo(info)
	writer.writeInfo(info)
	writer.writeGroups(reader.readGroups())
	writer.writeKerning(reader.readKerning())
	writer.writeLib(reader.readLib())
	if target_version >= 2:
		writer.writeFeatures(reader.readFeatures())
	src = reader.getGlyphSet()
	dst = writer.getGlyphSet()
	for glyphName in src.keys():
		g = src[glyphName]
		g.drawPoints(None)  # load attrs
		dst.writeGlyph(glyphName, g, g.drawPoints)
	dst.writeContents()
	writer.writeLayerContents()
	writer.close()


def convert():
	out = OUT / "convert"
	if out.exists():
		shutil.rmtree(out)
	out.mkdir(parents=True)
	for version in (1, 2, 3):
		for target_version in range(version, 4):
			convert_font(version, target_version, out / f"UFO{version} to {target_version}.ufo")


def roundtrip():
	out = OUT / "roundtrip"
	if out.exists():
		shutil.rmtree(out)
	out.mkdir(parents=True)
	with tempfile.TemporaryDirectory() as tmp:
		src = GlyphSet(str(DEMO), ufoFormatVersion=2, validateRead=True, validateWrite=True)
		dst = GlyphSet(tmp, ufoFormatVersion=2, validateRead=True, validateWrite=True)
		for glyphName in src.keys():
			g = src[glyphName]
			g.drawPoints(None)  # load attrs
			dst.writeGlyph(glyphName, g, g.drawPoints)
		for glyphName in sorted(src.keys()):
			fileName = src.contents[glyphName]
			shutil.copyfile(pathlib.Path(tmp) / fileName, out / fileName)


if __name__ == "__main__":
	roundtrip()
	convert()
