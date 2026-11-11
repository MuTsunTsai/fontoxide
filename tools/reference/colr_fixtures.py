"""Generates the fixtures of crates/fontoxide-ttlib/tests/data/colr.

fontTools' Tests/ttLib/tables/C_O_L_R_test.py keeps its sample data in Python
constants. This script saves them as text files:

- <NAME>.hex: the bytes of a `*_SAMPLE` table, as hex, one fragment per line.
- <NAME>.txt: an XML list (`COLR_V*_XML`), one item per line.

    uv run --project tools/reference python tools/reference/colr_fixtures.py
"""

import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "upstream" / "fonttools" / "Lib"))
sys.path.insert(0, str(ROOT / "upstream" / "fonttools" / "Tests" / "ttLib" / "tables"))
sys.dont_write_bytecode = True

import C_O_L_R_test as t

OUT = ROOT / "crates" / "fontoxide-ttlib" / "tests" / "data" / "colr"
OUT.mkdir(parents=True, exist_ok=True)

for name in ["COLR_V0_SAMPLE", "COLR_V1_SAMPLE"]:
    lines = [f"{b.hex()} {desc}" for b, desc in getattr(t, name)]
    (OUT / f"{name}.hex").write_text("\n".join(lines) + "\n", encoding="utf-8")

for name in ["COLR_V0_XML", "COLR_V1_XML", "COLR_V1_VAR_XML"]:
    (OUT / f"{name}.txt").write_text("\n".join(getattr(t, name)) + "\n", encoding="utf-8")
