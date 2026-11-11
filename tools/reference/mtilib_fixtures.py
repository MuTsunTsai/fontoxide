"""Generates crates/fontoxide-mtilib/tests/data/compiled.txt.

Tests/mtiLib/mti_test.py compares the TTX of the tables that mtiLib builds; this
script also records the binary tables fontTools compiles from the same files
(one line per test case: the test name and the hex of the compiled table), so
that the Rust tests can compare them byte for byte.

    uv run --project tools/reference python tools/reference/mtilib_fixtures.py
"""

import os
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "upstream" / "fonttools" / "Lib"))
sys.path.insert(0, str(ROOT / "upstream" / "fonttools" / "Tests" / "mtiLib"))
sys.dont_write_bytecode = True

os.environ["FONTTOOLS_LOOKUP_DEBUGGING"] = "1"

from fontTools import mtiLib  # noqa: E402
from mti_test import MtiTest  # noqa: E402

OUT = ROOT / "crates" / "fontoxide-mtilib" / "tests" / "data" / "compiled.txt"


def main():
	lines = []
	for tableTag, tests in MtiTest.TESTS.items():
		for name in tests:
			font = MtiTest.create_font()
			with open(MtiTest.getpath(name + ".txt"), "rt", encoding="utf-8") as f:
				table = mtiLib.build(f, font, tableTag=tableTag)
			blob = table.compile(font)
			test = "%s%s" % (name, "_" + tableTag if tableTag else "")
			lines.append("%s %s\n" % (test, blob.hex()))
	OUT.parent.mkdir(parents=True, exist_ok=True)
	OUT.write_text("".join(lines), encoding="utf-8", newline="\n")


if __name__ == "__main__":
	main()
