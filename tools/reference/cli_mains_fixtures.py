"""Generates the fixtures of the command lines of fontTools.pens.statisticsPen and
fontTools.designspaceLib, which have no upstream tests:

- crates/fontoxide-cli/tests/data/statistics_pen/<case>.txt: what
  `fontTools.pens.statisticsPen.main(args)` prints, for a few fonts and options;
- crates/fontoxide-designspacelib/tests/data/cli/<name>.designspace: what
  `fontTools.designspaceLib.main([input, output])` writes for a few upstream
  designspace files, copied to a directory where the output is written too
  (the relative paths of the output depend on its directory).

For both, help.txt is the help (`-h`), and error-<n>.txt the exit code and the
standard error of a usage error (argparse formats them for COLUMNS=80).

Run with:

    uv run --project tools/reference python tools/reference/cli_mains_fixtures.py
"""

import io
import os
import pathlib
import shutil
import sys
import tempfile
from contextlib import redirect_stderr, redirect_stdout

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "upstream" / "fonttools" / "Lib"))
sys.dont_write_bytecode = True
os.environ["COLUMNS"] = "80"

from fontTools import designspaceLib  # noqa: E402
from fontTools.pens import statisticsPen  # noqa: E402

TESTS = ROOT / "upstream" / "fonttools" / "Tests"
STATISTICS_OUT = ROOT / "crates" / "fontoxide-cli" / "tests" / "data" / "statistics_pen"
DESIGNSPACE_OUT = ROOT / "crates" / "fontoxide-designspacelib" / "tests" / "data" / "cli"

# The fonts are given relative to Tests/.
STATISTICS_CASES = {
    "I": ["ttLib/data/I.ttf"],
    "I-glyph-quiet": ["-q", "ttLib/data/I.ttf", "I"],
    "I-control-wght700": ["-q", "-c", "--variations", "wght=700", "ttLib/data/I.ttf"],
    "plex": ["ttLib/data/IBMPlexSans-Bold.subset.otf"],
    "plex-control": ["-c", "-q", "ttLib/data/IBMPlexSans-Bold.subset.otf"],
}

DESIGNSPACES = [
    "varLib/data/Build.designspace",
    "designspaceLib/data/test_v4_original.designspace",
    "designspaceLib/data/test_v5.designspace",
]

# The arguments of the usage errors.
STATISTICS_ERRORS = [[], ["-y"], ["--bogus", "a.ttf"], ["a.ttf", "-q", "--variations"]]
DESIGNSPACE_ERRORS = [[], ["a", "b", "c"]]


def write(path, text):
    path.write_text(text, encoding="utf-8", newline="\n")


def texts(main, out_dir, errors):
    """Saves the help and the usage errors of a `main`."""
    out = io.StringIO()
    with redirect_stdout(out):
        try:
            main(["-h"])
        except SystemExit:
            pass
    write(out_dir / "help.txt", out.getvalue())
    for i, args in enumerate(errors):
        err = io.StringIO()
        with redirect_stderr(err):
            try:
                main(args)
                code = 0
            except SystemExit as e:
                code = e.code
        write(out_dir / f"error-{i}.txt", f"{code}\n{err.getvalue()}")


def main():
    os.chdir(TESTS)
    STATISTICS_OUT.mkdir(parents=True, exist_ok=True)
    for name, args in STATISTICS_CASES.items():
        out = io.StringIO()
        with redirect_stdout(out):
            statisticsPen.main(args)
        write(STATISTICS_OUT / f"{name}.txt", out.getvalue())
    texts(statisticsPen.main, STATISTICS_OUT, STATISTICS_ERRORS)
    DESIGNSPACE_OUT.mkdir(parents=True, exist_ok=True)
    for path in DESIGNSPACES:
        name = pathlib.Path(path).name
        with tempfile.TemporaryDirectory() as tmp:
            source = pathlib.Path(tmp) / name
            output = pathlib.Path(tmp) / "output.designspace"
            shutil.copyfile(path, source)
            designspaceLib.main([str(source), str(output)])
            shutil.copyfile(output, DESIGNSPACE_OUT / name)
    texts(designspaceLib.main, DESIGNSPACE_OUT, DESIGNSPACE_ERRORS)


if __name__ == "__main__":
    main()
