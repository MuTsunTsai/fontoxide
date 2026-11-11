"""Generates crates/fontoxide-varlib/tests/data/mains/.

Runs the command lines (`main`) of fontTools.varLib.avar.build,
fontTools.varLib.avar.unbuild, fontTools.varLib.hvar, fontTools.varLib.models and
fontTools.varLib.varStore on upstream test data, with the clock fixed
(SOURCE_DATE_EPOCH=1700000000), so that the Rust tests (tests/mains.rs) can
compare their output byte for byte. The input fonts are compiled from upstream
TTX files.

    uv run --project tools/reference python tools/reference/varlib_mains_fixtures.py
"""

import contextlib
import io
import os
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "upstream" / "fonttools" / "Lib"))
sys.dont_write_bytecode = True
os.environ["SOURCE_DATE_EPOCH"] = "1700000000"

from fontTools.ttLib import TTFont  # noqa: E402
from fontTools.varLib import hvar, models, varStore  # noqa: E402
from fontTools.varLib.avar import build, unbuild  # noqa: E402

DATA = ROOT / "upstream" / "fonttools" / "Tests" / "varLib" / "data"
OUT = ROOT / "crates" / "fontoxide-varlib" / "tests" / "data" / "mains"


def compile_ttx(ttx, name):
    font = TTFont()
    font.importXML(str(ttx))
    font.save(str(OUT / name))


def stdout_of(main, args):
    out = io.StringIO()
    with contextlib.redirect_stdout(out):
        code = main(args)
    assert not code, code
    return out.getvalue()


def unbuild_main(args):
    """`unbuild.main(args)`, whose `unbuild` writes to the `sys.stdout` of its
    definition time."""
    (path,) = args
    font = TTFont(path)
    out = io.StringIO()
    unbuild.unbuild(font, f=out)
    return out.getvalue()


def write_text(name, text):
    (OUT / name).write_text(text, encoding="utf-8", newline="\n")


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    compile_ttx(DATA / "master_ttx_interpolatable_ttf" / "TestFamily-Master0.ttx", "master.ttf")
    compile_ttx(DATA / "MutatorSans_All_Variable.ttx", "MutatorSans.ttf")

    # varLib.avar.build: adds fvar and avar (version 1, then 2) to a static font.
    build.main([str(OUT / "master.ttf"), str(DATA / "Build.designspace"), "-o", str(OUT / "avar-build.ttf")])
    build.main([str(OUT / "master.ttf"), str(DATA / "BuildAvar2.designspace"), "-o", str(OUT / "avar2-build.ttf")])
    # varLib.avar.unbuild.
    write_text("avar-unbuild.txt", unbuild_main([str(OUT / "avar-build.ttf")]))
    write_text("avar2-unbuild.txt", unbuild_main([str(OUT / "avar2-build.ttf")]))
    # varLib.hvar.
    hvar.main([str(OUT / "MutatorSans.ttf"), "-o", str(OUT / "hvar.ttf")])
    # varLib.models.
    write_text("models-l.txt", stdout_of(models.main, ["-l", "0,0", "1,0", "0,1", "1,1", "0.5,0", "0,-1", "0.25,-0.5"]))
    write_text("models-d.txt", stdout_of(models.main, ["-d", str(DATA / "Build.designspace")]))
    # varLib.varStore.
    write_text(
        "varstore.txt",
        stdout_of(varStore.main, [str(OUT / "MutatorSans.ttf"), str(OUT / "varstore.ttf")]),
    )
    write_text(
        "varstore-q.txt",
        stdout_of(varStore.main, ["--quantization", "10", str(OUT / "MutatorSans.ttf")]),
    )


if __name__ == "__main__":
    main()
