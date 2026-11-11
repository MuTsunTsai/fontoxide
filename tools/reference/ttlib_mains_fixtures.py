"""Generates crates/fontoxide-ttlib/tests/data/mains/: the outputs of fontTools' command
lines cffLib.width, cffLib.CFF2ToCFF, ttLib.scaleUpem and ttLib.removeOverlaps, which
crates/fontoxide-ttlib/tests/mains.rs compares with fontoxide's.

The clock is fixed (SOURCE_DATE_EPOCH=1700000000) so that the fonts can be compared byte
for byte. The <case>.txt files hold the exit code, standard output and standard error of
the help and of usage errors. CFFToCFF2-1.CFF2.otf, the input of CFF2ToCFF, is what cffLib.CFFToCFF2 makes
of Tests/cffLib/data/CFFToCFF2-1.otf.

    uv run --project tools/reference python tools/reference/ttlib_mains_fixtures.py
"""

import contextlib
import io
import os
import pathlib
import shutil
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "upstream" / "fonttools" / "Lib"))
sys.dont_write_bytecode = True
os.environ["SOURCE_DATE_EPOCH"] = "1700000000"

from fontTools.cffLib import width  # noqa: E402
from fontTools.cffLib import CFF2ToCFF, CFFToCFF2  # noqa: E402
from fontTools.ttLib import removeOverlaps, scaleUpem  # noqa: E402

TESTS = ROOT / "upstream" / "fonttools" / "Tests"
OUT = ROOT / "crates" / "fontoxide-ttlib" / "tests" / "data" / "mains"


def stdout_of(main, args):
    out = io.StringIO()
    with contextlib.redirect_stdout(out):
        main(args)
    return out.getvalue()


# The help and usage errors: (case, main, args).
TEXT_CASES = [
    ("width-help", width.main, ["-h"]),
    ("width-no-args", width.main, []),
    ("CFF2ToCFF-help", CFF2ToCFF.main, ["-h"]),
    ("CFF2ToCFF-no-file", CFF2ToCFF.main, ["missing.otf"]),
    ("CFF2ToCFF-verbose-quiet", CFF2ToCFF.main, ["-v", "-q", "missing.otf"]),
    ("scaleUpem-help", scaleUpem.main, ["-h"]),
    ("scaleUpem-no-upem", scaleUpem.main, ["font.ttf"]),
    ("removeOverlaps-help", removeOverlaps.main, ["-h"]),
    ("removeOverlaps-no-output", removeOverlaps.main, ["a.ttf"]),
    ("removeOverlaps-unknown", removeOverlaps.main, ["a.ttf", "b.ttf", "--bogus"]),
]


def text_case(main, args):
    out, err = io.StringIO(), io.StringIO()
    code = 0
    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
        try:
            code = main(args) or 0
        except SystemExit as e:
            code = e.code or 0
    return f"exit {code}\n--- stdout\n{out.getvalue()}--- stderr\n{err.getvalue()}"


def main():
    OUT.mkdir(parents=True, exist_ok=True)

    for case, case_main, args in TEXT_CASES:
        (OUT / f"{case}.txt").write_text(
            text_case(case_main, args), encoding="utf-8", newline="\n"
        )

    # cffLib.width
    text = stdout_of(
        width.main,
        [
            str(TESTS / "ttLib" / "data" / "I.otf"),
            str(TESTS / "cffLib" / "data" / "LinLibertine_RBI.otf"),
        ],
    )
    text += stdout_of(width.main, ["-b", str(TESTS / "ttLib" / "data" / "I.otf")])
    (OUT / "width.txt").write_text(text, encoding="utf-8", newline="\n")

    with tempfile.TemporaryDirectory() as tmp:
        tmp = pathlib.Path(tmp)

        # cffLib.CFF2ToCFF
        CFFToCFF2.main(
            [
                str(TESTS / "cffLib" / "data" / "CFFToCFF2-1.otf"),
                "-o",
                str(OUT / "CFFToCFF2-1.CFF2.otf"),
            ]
        )
        CFF2ToCFF.main([str(OUT / "CFFToCFF2-1.CFF2.otf"), "-o", str(tmp / "cff.otf")])
        shutil.copyfile(tmp / "cff.otf", OUT / "CFFToCFF2-1.CFF.otf")

        # ttLib.scaleUpem
        scaleUpem.main(
            [
                str(TESTS / "ttLib" / "data" / "I.ttf"),
                "512",
                "--output-file",
                str(tmp / "scaled.ttf"),
            ]
        )
        shutil.copyfile(tmp / "scaled.ttf", OUT / "I-scaled.ttf")

        # ttLib.removeOverlaps
        removeOverlaps.main(
            [
                str(TESTS / "ttLib" / "data" / "IBMPlexSans-Bold.subset.otf"),
                str(tmp / "removed.otf"),
            ]
        )
        shutil.copyfile(tmp / "removed.otf", OUT / "IBMPlexSans-Bold.removeOverlaps.otf")


if __name__ == "__main__":
    main()
