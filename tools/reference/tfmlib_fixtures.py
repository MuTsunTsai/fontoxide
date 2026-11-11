"""Generates the fixtures of crates/fontoxide-misc/tests/data/tfm_lib.

For each TFM file of upstream Tests/tfmLib/data, saves <file>.txt with the
`repr` of every attribute of `tfmLib.TFM`, one `name=repr` line each, so that
the tests compare everything that is read, beyond the assertions of
Tests/tfmLib/tfmLib_test.py.

    uv run --project tools/reference python tools/reference/tfmlib_fixtures.py
"""

import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "upstream" / "fonttools" / "Lib"))
sys.dont_write_bytecode = True

from fontTools import tfmLib

DATA = ROOT / "upstream" / "fonttools" / "Tests" / "tfmLib" / "data"
OUT = ROOT / "crates" / "fontoxide-misc" / "tests" / "data" / "tfm_lib"

ATTRS = [
	"checksum",
	"designsize",
	"codingscheme",
	"fonttype",
	"family",
	"seven_bit_safe_flag",
	"face",
	"extraheader",
	"fontdimens",
	"right_boundary_char",
	"left_boundary_char",
	"kerning",
	"ligatures",
	"chars",
]


def main():
	OUT.mkdir(parents=True, exist_ok=True)
	for path in sorted(DATA.glob("*.tfm")):
		tfm = tfmLib.TFM(str(path))
		lines = [f"{name}={getattr(tfm, name)!r}" for name in ATTRS]
		lines.append(repr(tfm))
		(OUT / (path.name + ".txt")).write_text("\n".join(lines) + "\n", newline="\n")


if __name__ == "__main__":
	main()
