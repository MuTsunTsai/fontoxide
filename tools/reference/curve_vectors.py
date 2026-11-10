"""Generates the cu2qu and qu2cu differential test vectors:

- crates/fontoxide-cu2qu/tests/data/cu2qu_vectors.txt
- crates/fontoxide-qu2cu/tests/data/qu2cu_vectors.txt

Converts the curves of fontTools' Tests/cu2qu/data/curves.json, and random ones,
with the pure Python fontTools (not compiled with Cython).

    uv run --project tools/reference python tools/reference/curve_vectors.py

Points are written as "x,y" with repr(float), separated by spaces; curves by " | ".
Each line has, separated by tabs:

- cu2qu: S (curve_to_quadratic) or M (curves_to_quadratic); the tolerances,
  separated by commas; a (all_quadratic) or c (all_quadratic=False); the curves;
  the result, or !E if it raises.
- qu2cu: Q; the tolerance; c (all_cubic) or q; the splines; the result, or !E.
"""

import json
import pathlib
import random
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "upstream" / "fonttools" / "Lib"))
sys.dont_write_bytecode = True

from fontTools.cu2qu import cu2qu as cu2qu_module  # noqa: E402
from fontTools.cu2qu import curve_to_quadratic, curves_to_quadratic  # noqa: E402
from fontTools.qu2cu import qu2cu as qu2cu_module  # noqa: E402
from fontTools.qu2cu import quadratic_to_curves  # noqa: E402

assert not cu2qu_module.COMPILED and not qu2cu_module.COMPILED

CU2QU_OUT = ROOT / "crates" / "fontoxide-cu2qu" / "tests" / "data" / "cu2qu_vectors.txt"
QU2CU_OUT = ROOT / "crates" / "fontoxide-qu2cu" / "tests" / "data" / "qu2cu_vectors.txt"


def pts(points):
    return " ".join(f"{float(x)!r},{float(y)!r}" for x, y in points)


def curves_str(curves):
    return " | ".join(pts(c) for c in curves)


def main():
    curves = json.loads((ROOT / "upstream/fonttools/Tests/cu2qu/data/curves.json").read_text())
    rng = random.Random(4680)

    def random_curve():
        kind = rng.random()
        if kind < 0.4:
            return [(rng.randint(-1000, 1000), rng.randint(-1000, 1000)) for _ in range(4)]
        if kind < 0.7:
            return [(round(rng.uniform(-500, 500), 3), round(rng.uniform(-500, 500), 3)) for _ in range(4)]
        # Degenerate: repeated or collinear points.
        a = (rng.randint(-100, 100), rng.randint(-100, 100))
        b = (rng.randint(-100, 100), rng.randint(-100, 100))
        mid = ((a[0] + b[0]) / 2, (a[1] + b[1]) / 2)
        return rng.choice([[a, a, a, a], [a, a, a, b], [a, mid, mid, b], [a, a, b, b], [a, b, a, b]])

    lines = []

    def single(curve, tol, all_quadratic):
        try:
            result = pts(curve_to_quadratic(curve, tol, all_quadratic))
        except Exception:
            result = "!E"
        flag = "a" if all_quadratic else "c"
        lines.append(f"S\t{float(tol)!r}\t{flag}\t{pts(curve)}\t{result}")

    def multi(group, tols, all_quadratic):
        try:
            result = curves_str(curves_to_quadratic(group, tols, all_quadratic))
        except Exception:
            result = "!E"
        flag = "a" if all_quadratic else "c"
        tol_str = ",".join(repr(float(t)) for t in tols)
        lines.append(f"M\t{tol_str}\t{flag}\t{curves_str(group)}\t{result}")

    for c in curves[:250]:
        single(c, 1.0, True)
    for c in curves[250:310]:
        single(c, 5.0, True)
        single(c, 0.1, True)
        single(c, 1.0, False)
    for _ in range(200):
        single(random_curve(), rng.choice([0.01, 0.1, 0.5, 1.0, 5.0]), rng.random() < 0.8)
    for i in range(0, 120, 3):
        multi(curves[i : i + 3], [5.0] * 3, True)
    for _ in range(40):
        group = [random_curve() for _ in range(rng.randint(1, 4))]
        multi(group, [rng.choice([0.1, 1.0, 5.0]) for _ in group], rng.random() < 0.8)
    CU2QU_OUT.parent.mkdir(parents=True, exist_ok=True)
    CU2QU_OUT.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")

    lines = []

    def q2c(splines, tol, all_cubic):
        try:
            result = curves_str(quadratic_to_curves(splines, tol, all_cubic))
        except Exception:
            result = "!E"
        flag = "c" if all_cubic else "q"
        lines.append(f"Q\t{float(tol)!r}\t{flag}\t{curves_str(splines)}\t{result}")

    # The roundtrip of Tests/qu2cu/qu2cu_test.py, and more tolerances.
    for c in curves[:150]:
        q2c([curve_to_quadratic(c, 1.0)], 1.0, False)
    for c in curves[400:450]:
        spline = curve_to_quadratic(c, 0.1)
        q2c([spline], 0.5, False)
        q2c([spline], 0.5, True)
    # Connected splines, as qu2cu's main converts contours.
    for i in range(550, 670, 3):
        splines = []
        start = None
        for c in curves[i : i + 3]:
            c = [tuple(p) for p in c]
            if start is not None:
                dx, dy = start[0] - c[0][0], start[1] - c[0][1]
                c = [(x + dx, y + dy) for x, y in c]
            spline = curve_to_quadratic(c, 1.0)
            splines.append(spline)
            start = spline[-1]
        q2c(splines, rng.choice([0.5, 1.0, 2.0]), rng.random() < 0.3)
    QU2CU_OUT.parent.mkdir(parents=True, exist_ok=True)
    QU2CU_OUT.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
