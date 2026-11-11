"""Generates the fixtures of crates/fontoxide-t1lib/tests/data.

For each Type 1 font of upstream Tests/t1Lib/data, this saves what fontTools
makes of it, for byte-for-byte comparisons beyond the assertions of
Tests/t1Lib/t1Lib_test.py:

- <file>.raw: the raw data (`T1Font(path).getData()`, `kind="LWFN"` for .lwfn).
- <file>.decrypted: `t1Lib.decryptType1` of the raw data.
- <file>.pfb, <file>.pfa, <file>.hex.pfa: the raw data written with
  `t1Lib.write` as PFB, and as OTHER in binary and in hexadecimal.
- <file>.created: the data made from the parsed font (`parse()`, then
  `createData()`).
- <file>.glyphs: every glyph drawn with a RecordingPen after parsing: a line
  `glyph <name> <width>`, then one line per pen call with its points.
- edited-ellipsis.created: the data of Tests/t1Lib/t1Lib_test.py::EditTest, the
  regular font with hints added to "ellipsis".

A failure is saved as <output>.err with the exception name.

    uv run --project tools/reference python tools/reference/t1lib_fixtures.py
"""

import os
import pathlib
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "upstream" / "fonttools" / "Lib"))
sys.dont_write_bytecode = True

from fontTools import t1Lib
from fontTools.pens.recordingPen import RecordingPen

DATA = ROOT / "upstream" / "fonttools" / "Tests" / "t1Lib" / "data"
OUT = ROOT / "crates" / "fontoxide-t1lib" / "tests" / "data"

FILES = [
	"TestT1-Regular.pfa",
	"TestT1-Regular.pfb",
	"TestT1-Regular.lwfn",
	"TestT1-weird-zeros.pfa",
	"TestT1-ellipsis-hinted.pfa",
]


def save(name, make):
	for stale in (OUT / name, OUT / (name + ".err")):
		if stale.exists():
			stale.unlink()
	try:
		data = make()
	except Exception as e:
		(OUT / (name + ".err")).write_text(type(e).__name__ + "\n")
	else:
		(OUT / name).write_bytes(data)


def num(v):
	return repr(float(v))


def open_font(file):
	kind = "LWFN" if file.endswith(".lwfn") else None
	return t1Lib.T1Font(DATA / file, kind=kind)


def written(data, kind, dohex):
	with tempfile.TemporaryDirectory() as tmp:
		path = os.path.join(tmp, "out")
		t1Lib.write(path, data, kind, dohex)
		with open(path, "rb") as f:
			return f.read()


def glyphs(file):
	font = open_font(file)
	font.parse()
	glyph_set = font.getGlyphSet()
	lines = []
	for name in glyph_set.keys():
		glyph = glyph_set[name]
		pen = RecordingPen()
		glyph.draw(pen)
		lines.append(f"glyph {name} {num(glyph.width)}")
		for op, args in pen.value:
			if op == "addComponent":
				base, t = args
				lines.append(f"{op} {base} " + " ".join(num(v) for v in t))
			else:
				lines.append(op + "".join(f" {num(x)} {num(y)}" for x, y in args))
	return ("\n".join(lines) + "\n").encode()


def created(file):
	font = open_font(file)
	font.parse()
	return font.createData()


def edited():
	font = t1Lib.T1Font(DATA / "TestT1-Regular.pfa")
	ellipsis = font.getGlyphSet()["ellipsis"]
	ellipsis.decompile()
	program = []
	for v in ellipsis.program:
		try:
			program.append(int(v))
		except:
			program.append(v)
			if v == "hsbw":
				hints = [55, 131, 296, 131, 537, 131, "vstem3", 0, 122, "hstem"]
				program.extend(hints)
	ellipsis.program = program
	return font.getData()


def main():
	OUT.mkdir(parents=True, exist_ok=True)
	for file in FILES:
		raw = open_font(file).getData()
		save(file + ".raw", lambda: raw)
		save(file + ".decrypted", lambda: t1Lib.decryptType1(raw))
		save(file + ".pfb", lambda: written(raw, "PFB", False))
		save(file + ".pfa", lambda: written(raw, "OTHER", False))
		save(file + ".hex.pfa", lambda: written(raw, "OTHER", True))
		save(file + ".created", lambda: created(file))
		save(file + ".glyphs", lambda: glyphs(file))
	save("edited-ellipsis.created", edited)


if __name__ == "__main__":
	main()
