"""Writes fonts with the beyond-64k companion tables for the parity corpus.

Run with the reference environment, in isolated mode so that the reference fontTools comes
from the submodule path given by reference.py's setup:

	uv run --project tools/reference python -I tools/corpus/beyond64k_fonts.py corpus/beyond64k <fonts>…

Each of the given fonts is converted with `upper_tables`. Besides those, this writes a font
that really has more than 65536 glyphs, with composite glyphs, layout and variations that
refer to glyph IDs above 65535.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "upstream" / "fonttools" / "Lib"))

from fontTools.feaLib.builder import addOpenTypeFeaturesFromString  # noqa: E402
from fontTools.fontBuilder import FontBuilder  # noqa: E402
from fontTools.pens.ttGlyphPen import TTGlyphPen  # noqa: E402
from fontTools.ttLib import TTFont  # noqa: E402
from fontTools.ttLib.beyond64k import upper_tables  # noqa: E402
from fontTools.ttLib.tables._g_v_a_r import TupleVariation  # noqa: E402

GLYPH_COUNT = 0x10000 + 200


def square(size):
	pen = TTGlyphPen(None)
	pen.moveTo((0, 0))
	pen.lineTo((size, 0))
	pen.lineTo((size, size))
	pen.lineTo((0, size))
	pen.closePath()
	return pen.glyph()


def composite(*names):
	pen = TTGlyphPen(dict.fromkeys(names))
	for i, name in enumerate(names):
		pen.addComponent(name, (1, 0, 0, 1, i * 100, 0))
	return pen.glyph()


def build_large_font(variable):
	names = [".notdef"] + [f"g{i:05X}" for i in range(1, GLYPH_COUNT)]
	high = names[0x10000:]
	builder = FontBuilder(1000, isTTF=True, beyond64k=True)
	builder.setupGlyphOrder(names)
	builder.setupCharacterMap({0x41: names[1], 0x42: high[0], 0x1F600: high[1], 0x43: high[2]})
	glyphs = {name: square(100 + i % 400) for i, name in enumerate(names)}
	# Components above 65535 need the 24-bit glyph IDs.
	glyphs[high[2]] = composite(high[0], high[1])
	glyphs[names[2]] = composite(high[3], names[1])
	builder.setupGlyf(glyphs)
	builder.setupHorizontalMetrics({name: (500 + i % 7, 0) for i, name in enumerate(names)})
	builder.setupHorizontalHeader(ascent=800, descent=-200)
	builder.setupNameTable({"familyName": "Beyond", "styleName": "Regular"})
	builder.setupOS2()
	builder.setupPost()
	addOpenTypeFeaturesFromString(
		builder.font,
		f"""
		languagesystem DFLT dflt;
		@high = [{" ".join(high[:60])}];
		feature liga {{ sub {high[0]} {high[1]} by {high[2]}; }} liga;
		feature ss01 {{ sub {names[1]} by {high[4]}; }} ss01;
		feature calt {{ sub {names[1]}' {high[0]} by {high[5]}; }} calt;
		feature kern {{
			pos {high[0]} {high[1]} -50;
			pos @high {names[1]} 20;
		}} kern;
		feature mark {{
			markClass [{high[6]}] <anchor 10 20> @TOP;
			pos base [{high[7]} {names[1]}] <anchor 100 200> mark @TOP;
		}} mark;
		""",
	)
	if variable:
		builder.setupFvar([("wght", 100, 400, 900, "Weight")], [])
		delta = [(10, 0), (10, 0), (10, 10), (0, 10)] + [(0, 0)] * 4
		variations = {
			names[1]: [TupleVariation({"wght": (0, 1, 1)}, delta)],
			high[0]: [TupleVariation({"wght": (-1, -1, 0)}, delta)],
		}
		builder.setupGvar(variations)
	return builder.font


def main():
	out = Path(sys.argv[1])
	out.mkdir(parents=True, exist_ok=True)
	for path in map(Path, sys.argv[2:]):
		try:
			font = TTFont(path, recalcTimestamp=False)
			upper_tables(font)
			font.save(out / f"upper-{path.stem}{path.suffix}")
		except Exception as e:
			print(f"{path}: {e!r}")
	for variable in (False, True):
		font = build_large_font(variable)
		name = "large-variable.ttf" if variable else "large.ttf"
		font.save(out / name)
		print(name, len(font.getGlyphOrder()), sorted(font.keys())[:40])


if __name__ == "__main__":
	main()
