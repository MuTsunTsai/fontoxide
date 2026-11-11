"""Merges fonts with fontTools.merge, as the reference for crates/fontoxide-merge.

Usage:

    uv run --project tools/reference python tools/reference/merge_fixtures.py \
        OUT FONT1 FONT2 [...] [--drop-tables=...]

Runs `fonttools merge` on the fonts (and any merge options), writes the merged
font to OUT.ttf and its TTX dump to OUT.ttx, and prints how long merging and
saving took (the best of three runs). Set PYTHONHASHSEED=0 and SOURCE_DATE_EPOCH
(for `head.created` and `head.modified`) to get reproducible output; the merged
font of `cargo run -p fontoxide-merge --example merge` with the same
SOURCE_DATE_EPOCH must then be identical.
"""

import logging
import pathlib
import sys
import time

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "upstream" / "fonttools" / "Lib"))
sys.dont_write_bytecode = True

from fontTools.merge import Merger, Options  # noqa: E402
from fontTools.ttLib import TTFont  # noqa: E402


def main(args):
    out = pathlib.Path(args[0])
    options = Options()
    fontfiles = options.parse_opts(args[1:])
    logging.basicConfig(level=logging.WARNING)
    best = None
    for _ in range(3):
        start = time.perf_counter()
        font = Merger(options=options).merge(fontfiles)
        font.save(str(out.with_suffix(".ttf")))
        elapsed = time.perf_counter() - start
        best = elapsed if best is None else min(best, elapsed)
    TTFont(str(out.with_suffix(".ttf"))).saveXML(str(out.with_suffix(".ttx")))
    print(f"{out.name}: {best * 1000:.0f} ms")


if __name__ == "__main__":
    main(sys.argv[1:])
