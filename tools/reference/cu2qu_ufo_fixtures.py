"""Generates crates/fontoxide-cu2qu/tests/data/cli_outputs.txt.

Runs the cu2qu command line of fontTools (with ufoLib2, as the upstream tests
do) on copies of the UFOs of Tests/cu2qu/data in several modes, and records a
digest of every file it writes, so that the Rust tests can check that the
output is byte for byte the same without storing the UFOs.

    uv run --project tools/reference python tools/reference/cu2qu_ufo_fixtures.py

Each line has, separated by tabs: the mode, the path of the file relative to
the output directory (with "/"), its size and its 64-bit FNV-1a hash in hex.
"""

import pathlib
import shutil
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "upstream" / "fonttools" / "Lib"))
sys.dont_write_bytecode = True

from fontTools.cu2qu.cli import _main  # noqa: E402

DATA = ROOT / "upstream" / "fonttools" / "Tests" / "cu2qu" / "data"
UFOS = ["RobotoSubset-Regular.ufo", "RobotoSubset-Bold.ufo"]
OUT = ROOT / "crates" / "fontoxide-cu2qu" / "tests" / "data" / "cli_outputs.txt"

# mode name -> (arguments before the inputs, whether to write to -d out)
MODES = {
    "default": ([], True),
    "interpolatable": (["-i"], True),
    "mixed": (["-i", "-m", "--keep-direction", "-e", "0.002"], True),
    "jobs": (["-j", "2", "-e", "0.003"], True),
    "inplace": ([], False),
    "inplace-interpolatable": (["-i", "--keep-direction"], False),
}


def fnv1a64(data):
    h = 0xCBF29CE484222325
    for b in data:
        h ^= b
        h = (h * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return h


def main():
    lines = []
    for mode, (args, to_dir) in MODES.items():
        with tempfile.TemporaryDirectory() as tmp:
            tmp = pathlib.Path(tmp)
            src = tmp / "src"
            src.mkdir()
            inputs = []
            for name in UFOS:
                shutil.copytree(DATA / name, src / name)
                inputs.append(str(src / name))
            if to_dir:
                out = tmp / "out"
                _main(args + ["-d", str(out)] + inputs)
            else:
                out = src
                _main(args + inputs)
            for path in sorted(p for p in out.rglob("*") if p.is_file()):
                data = path.read_bytes()
                rel = path.relative_to(out).as_posix()
                lines.append(f"{mode}\t{rel}\t{len(data)}\t{fnv1a64(data):016x}")
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
