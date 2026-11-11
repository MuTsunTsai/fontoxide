"""Generates the fixture of the CFFToCFF2Test port in crates/fontoxide-ttlib/tests/cff2_to_cff.rs.

Converts Tests/cffLib/data/CFFToCFF2-1.otf to CFF2 as test_conversion does, saves the font and
reads it back, and writes the TTX of its CFF2 table, to compare the conversion with.

    uv run --project tools/reference python tools/reference/cff_to_cff2_fixtures.py
"""

import io
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "upstream" / "fonttools" / "Lib"))
sys.dont_write_bytecode = True

from fontTools.cffLib.CFFToCFF2 import convertCFFToCFF2
from fontTools.ttLib import TTFont

DATA = ROOT / "upstream" / "fonttools" / "Tests" / "cffLib" / "data"
OUT = ROOT / "crates" / "fontoxide-ttlib" / "tests" / "data"


def main():
    font = TTFont(DATA / "CFFToCFF2-1.otf")
    convertCFFToCFF2(font)
    buf = io.BytesIO()
    font.save(buf)
    buf.seek(0)
    font = TTFont(buf)
    out = io.StringIO()
    font.saveXML(out, tables=["CFF2"], newlinestr="\n")
    lines = [l for l in out.getvalue().splitlines() if not l.startswith("<ttFont ")]
    (OUT / "CFFToCFF2-1.CFF2.ttx").write_text("\n".join(lines) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
