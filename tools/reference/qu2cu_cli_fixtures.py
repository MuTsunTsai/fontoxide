"""Generates crates/fontoxide-qu2cu/tests/data/{cubic,all-cubic}.ttf.

Runs the qu2cu command line of fontTools on Tests/qu2cu/data's font, without
and with -c, with the clock fixed (SOURCE_DATE_EPOCH=1700000000) so that the
Rust tests can compare their output byte for byte.

    uv run --project tools/reference python tools/reference/qu2cu_cli_fixtures.py
"""

import os
import pathlib
import shutil
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "upstream" / "fonttools" / "Lib"))
sys.dont_write_bytecode = True
os.environ["SOURCE_DATE_EPOCH"] = "1700000000"

from fontTools.qu2cu.cli import _main  # noqa: E402

FONT = ROOT / "upstream" / "fonttools" / "Tests" / "qu2cu" / "data" / "NotoSansArabic-Regular.quadratic.subset.ttf"
OUT = ROOT / "crates" / "fontoxide-qu2cu" / "tests" / "data"


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    for name, args in (("cubic.ttf", []), ("all-cubic.ttf", ["-c"])):
        with tempfile.TemporaryDirectory() as tmp:
            out = pathlib.Path(tmp) / name
            _main([str(FONT), "-o", str(out)] + args)
            shutil.copyfile(out, OUT / name)


if __name__ == "__main__":
    main()
