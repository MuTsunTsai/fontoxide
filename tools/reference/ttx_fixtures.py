"""Generates the fixtures of crates/fontoxide-cli/tests/data/ttx.

Runs fontTools' `ttx` command line (`fontTools.ttx.main`) on the test fonts of
Tests/ttx/data and saves what it writes, which the tests of fontoxide-cli
compare byte for byte with the output of `fontoxide_cli::ttx::main`:

- dump_<case>/: the TTX files of dumping a font with some options, including
  the table and glyph files of `-s` and `-g`.
- list_<font>.txt: the output of `-l`.
- compile_<case>.bin: the font compiled from a TTX file with some options (with
  `--no-recalc-timestamp`, so that the result does not depend on the
  modification time of the TTX file).

    uv run --project tools/reference python tools/reference/ttx_fixtures.py
"""

import contextlib
import io
import pathlib
import shutil
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "upstream" / "fonttools" / "Lib"))
sys.dont_write_bytecode = True

from fontTools import ttx

DATA = ROOT / "upstream" / "fonttools" / "Tests" / "ttx" / "data"
OUT = ROOT / "crates" / "fontoxide-cli" / "tests" / "data" / "ttx"

# (case, input file, options)
DUMPS = [
	("ttf", "TestTTF.ttf", []),
	("ttf_s", "TestTTF.ttf", ["-s"]),
	("ttf_g", "TestTTF.ttf", ["-g"]),
	("ttf_i", "TestTTF.ttf", ["-i"]),
	("ttf_crlf", "TestTTF.ttf", ["--newline", "CRLF"]),
	("ttf_t", "TestTTF.ttf", ["-t", "cmap", "-t", "glyf", "-t", "GlyphOrder"]),
	("ttf_x", "TestTTF.ttf", ["-x", "glyf", "-x", "GlyphOrder"]),
	("ttf_g_t", "TestTTF.ttf", ["-g", "-t", "glyf", "-t", "head"]),
	("otf", "TestOTF.otf", []),
	("otf_s", "TestOTF.otf", ["-s"]),
	("woff", "TestWOFF.woff", []),
	("woff2", "TestWOFF2.woff2", []),
	("ttc_1", "TestTTC.ttc", ["-y", "1"]),
]

LISTS = ["TestTTF.ttf", "TestOTF.otf", "TestWOFF.woff", "TestWOFF2.woff2"]

# (case, input file, options)
COMPILES = [
	("ttf", "TestTTF.ttx", []),
	("ttf_b", "TestTTF.ttx", ["-b"]),
	("ttf_woff", "TestTTF.ttx", ["--flavor", "woff"]),
	("ttf_woff2", "TestTTF.ttx", ["--flavor", "woff2"]),
	("otf", "TestOTF.ttx", []),
	("otf_woff", "TestOTF.ttx", ["--flavor", "woff"]),
	("otf_woff2", "TestOTF.ttx", ["--flavor", "woff2"]),
]


def main():
	if OUT.exists():
		shutil.rmtree(OUT)
	OUT.mkdir(parents=True)
	(OUT / ".gitattributes").write_text(
		"# The TTX files of `--newline CRLF` keep their line endings.\n"
		"dump_ttf_crlf/* -text\n",
		newline="\n",
	)
	for case, font, options in DUMPS:
		folder = OUT / f"dump_{case}"
		folder.mkdir()
		output = folder / (pathlib.Path(font).stem + ".ttx")
		ttx.main(["-q", *options, "-o", str(output), str(DATA / font)])
	for font in LISTS:
		out = io.StringIO()
		with contextlib.redirect_stdout(out):
			options = ttx.Options([], 1)
			ttx.ttList(str(DATA / font), "", options)
		# The path of the input is replaced by its file name.
		text = out.getvalue().replace(str(DATA / font), font)
		(OUT / f"list_{pathlib.Path(font).stem}.txt").write_text(text, newline="\n")
	for case, font, options in COMPILES:
		output = OUT / f"compile_{case}.bin"
		ttx.main(["-q", "--no-recalc-timestamp", *options, "-o", str(output), str(DATA / font)])


if __name__ == "__main__":
	main()
