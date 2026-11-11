"""Reference COLR tables built by fontTools.colorLib, for fontoxide-colorlib tests.

For each case, writes the TTX of the table as built (before compiling, which
sorts the base glyph records) and the compiled table as hex:

	uv run --project tools/reference python tools/reference/colorlib_fixtures.py \
		crates/fontoxide-colorlib/tests/data

The inputs are mirrored by `crates/fontoxide-colorlib/tests/fixtures.rs`.
"""

import sys
from io import StringIO
from pathlib import Path

sys.dont_write_bytecode = True
UPSTREAM = Path(__file__).resolve().parents[2] / "upstream" / "fonttools"
sys.path.insert(0, str(UPSTREAM / "Lib"))
sys.path.insert(0, str(UPSTREAM / "Tests"))

from fontTools.colorLib.builder import buildCOLR  # noqa: E402
from fontTools.misc.xmlWriter import XMLWriter  # noqa: E402
from fontTools.ttLib import TTFont  # noqa: E402
from fontTools.ttLib.tables import otTables as ot  # noqa: E402

from colorLib.unbuilder_test import TEST_COLOR_GLYPHS  # noqa: E402

F = ot.PaintFormat


def unbuilder_case():
	order = [".notdef"] + [f"glyph{i:05}" for i in range(1, 18)]
	order.append("glyph{str(18 + i).zfill(5)}")
	return order, TEST_COLOR_GLYPHS, {}


def mixed_case():
	order = [".notdef"] + list("abcdefghij")
	solid = {"Format": F.PaintSolid, "PaletteIndex": 2, "Alpha": 0.8}
	paints = [
		{"Format": F.PaintGlyph, "Paint": solid, "Glyph": c} for c in "abcdefgh"
	]
	color_glyphs = {
		"j": [("a", 0), ("b", 1)],
		"c": (F.PaintColrLayers, paints[0:4]),
		"b": (F.PaintColrLayers, paints[0:6]),
		"d": (F.PaintColrLayers, paints[2:8]),
		"i": {
			"Format": F.PaintGlyph,
			"Glyph": "i",
			"Paint": {
				"Format": F.PaintVarRadialGradient,
				"ColorLine": {
					"Extend": "reflect",
					"ColorStop": [(0.0, 3), (0.5, 4, 0.25, 7), (1.0, 5)],
				},
				"x0": 1.4,
				"y0": 0,
				"r0": 0,
				"x1": 2.6,
				"y1": 0,
				"r1": 1.3,
				"VarIndexBase": 9,
			},
		},
		"h": {
			"Format": F.PaintComposite,
			"SourcePaint": {"Format": F.PaintColrGlyph, "Glyph": "c"},
			"CompositeMode": "multiply",
			"BackdropPaint": {
				"Format": F.PaintVarTransform,
				"Paint": (F.PaintGlyph, (F.PaintSolid, 1), "e"),
				"Transform": (1, 0.3333, -0.5, 1.25, 100.5, -20, 4),
			},
		},
		"g": (
			F.PaintColrLayers,
			[
				{
					"Format": F.PaintSweepGradient,
					"ColorLine": {"ColorStop": [(0.0, 0), (1.0, 1, 0.5)]},
					"centerX": 10.6,
					"centerY": -3,
					"startAngle": 45,
					"endAngle": 270.5,
				},
				{
					"Format": F.PaintRotateAroundCenter,
					"Paint": (F.PaintColrGlyph, "b"),
					"angle": -30.25,
					"centerX": 5,
					"centerY": 6,
				},
				{
					"Format": F.PaintVarSkewAroundCenter,
					"Paint": paints[0],
					"xSkewAngle": 10,
					"ySkewAngle": 0.5,
					"centerX": 0,
					"centerY": 0,
				},
			],
		),
	}
	clip_boxes = {
		"c": (-100.8, -200.4, 1100.1, 1200.5),
		"b": (0, 0, 1000, 1000, 3),
		"i": (0, 0, 10, 10),
		"d": (0, 0, 10, 10),
	}
	return order, color_glyphs, clip_boxes


CASES = {
	"unbuilder": unbuilder_case,
	"mixed": mixed_case,
}


def main(out_dir: str) -> None:
	out = Path(out_dir)
	out.mkdir(parents=True, exist_ok=True)
	for name, case in CASES.items():
		order, color_glyphs, clip_boxes = case()
		font = TTFont()
		font.setGlyphOrder(order)
		colr = buildCOLR(color_glyphs, clipBoxes=clip_boxes or None)
		font["COLR"] = colr
		buf = StringIO()
		writer = XMLWriter(buf, indentwhite="  ", newlinestr="\n")
		colr.toXML(writer, font)
		xml = buf.getvalue()
		data = colr.compile(font)
		(out / f"{name}.xml").write_text(xml, encoding="utf-8", newline="\n")
		(out / f"{name}.hex").write_text(data.hex() + "\n", encoding="utf-8", newline="\n")


if __name__ == "__main__":
	main(sys.argv[1])
