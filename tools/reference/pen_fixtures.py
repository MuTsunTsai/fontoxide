"""Generates tests/data/cu2qu_glyphs.txt of crates/fontoxide-cu2qu and
crates/fontoxide-qu2cu (the same file, for their pen tests).

The cu2quPen and qu2cuPen tests of fontTools draw the UFO glyphs of
Tests/pens/data/{cubic,quadratic} with ufoLib's GlyphSet, which fontoxide does
not have yet; this records them instead.

    uv run --project tools/reference python tools/reference/pen_fixtures.py

Each line has, separated by tabs: the glyph set (cubic or quadratic), the glyph
name, "S" or "P", and the calls of `draw` (S) or `drawPoints` (P) separated by
"; ":

- S: `M x y`, `L x y`, `C x y …`, `Q x y … [_]`, `Z`, `E`, `A name xx xy yx yy dx dy`;
- P: `B`, `E`, `P x y type smooth name` (type and name `-` for None, smooth 0
  or 1), `A name xx xy yx yy dx dy`.
"""

import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "upstream" / "fonttools" / "Lib"))
sys.dont_write_bytecode = True

from fontTools.pens.recordingPen import RecordingPen, RecordingPointPen  # noqa: E402
from fontTools.ufoLib.glifLib import GlyphSet  # noqa: E402

DATA = ROOT / "upstream" / "fonttools" / "Tests" / "pens" / "data"
OUTS = [
    ROOT / "crates" / crate / "tests" / "data" / "cu2qu_glyphs.txt"
    for crate in ("fontoxide-cu2qu", "fontoxide-qu2cu")
]


def num(v):
    return repr(float(v))


def segment_ops(recording):
    out = []
    for op, args in recording:
        if op == "moveTo":
            out.append("M " + " ".join(num(c) for c in args[0]))
        elif op == "lineTo":
            out.append("L " + " ".join(num(c) for c in args[0]))
        elif op in ("curveTo", "qCurveTo"):
            pts = ["_" if p is None else f"{num(p[0])} {num(p[1])}" for p in args]
            out.append(("C " if op == "curveTo" else "Q ") + " ".join(pts))
        elif op == "closePath":
            out.append("Z")
        elif op == "endPath":
            out.append("E")
        elif op == "addComponent":
            out.append("A " + args[0] + " " + " ".join(num(c) for c in args[1]))
        else:
            raise ValueError(op)
    return "; ".join(out)


def point_ops(recording):
    out = []
    for op, args, kwargs in recording:
        if op == "beginPath":
            out.append("B")
        elif op == "endPath":
            out.append("E")
        elif op == "addPoint":
            (x, y), segment_type, smooth, name = args
            out.append(f"P {num(x)} {num(y)} {segment_type or '-'} {int(bool(smooth))} {name or '-'}")
        elif op == "addComponent":
            out.append("A " + args[0] + " " + " ".join(num(c) for c in args[1]))
        else:
            raise ValueError(op)
    return "; ".join(out)


MEASURE_OUT = ROOT / "crates" / "fontoxide-pens" / "tests" / "data" / "measure_vectors.txt"


def measure_vectors():
    """The values of the measuring pens on recorded and random outlines.

    Each line: the outline (segment calls as above), then the values separated
    by tabs, with repr(float), or !E if the pen raises:
    moments (area momentX momentY momentXX momentXY momentYY), statistics
    (area meanX meanY varianceX varianceY stddevX stddevY covariance
    correlation slant), control statistics (same), area, perimeter at
    tolerance 0.005 and 0.0001, bounds and control bounds (or "None").
    """
    import random

    from fontTools.pens.areaPen import AreaPen
    from fontTools.pens.boundsPen import BoundsPen, ControlBoundsPen
    from fontTools.pens.momentsPen import MomentsPen
    from fontTools.pens.perimeterPen import PerimeterPen
    from fontTools.pens.statisticsPen import StatisticsControlPen, StatisticsPen
    from fontTools.pens import momentsPen as moments_module

    assert not moments_module.COMPILED

    outlines = []
    for kind in ("cubic", "quadratic"):
        glyphs = GlyphSet(str(DATA / kind))
        for name in sorted(glyphs.keys()):
            rec = RecordingPen()
            glyphs[name].draw(rec)
            if not any(op == "addComponent" for op, _ in rec.value):
                outlines.append(rec.value)
    rng = random.Random(4680)

    def pt(scale):
        if rng.random() < 0.5:
            return (rng.randint(-scale, scale), rng.randint(-scale, scale))
        return (round(rng.uniform(-scale, scale), 2), round(rng.uniform(-scale, scale), 2))

    for _ in range(150):
        scale = rng.choice([10, 100, 1000])
        rec = RecordingPen()
        for _ in range(rng.randint(1, 3)):
            rec.moveTo(pt(scale))
            for _ in range(rng.randint(1, 6)):
                r = rng.random()
                if r < 0.3:
                    rec.lineTo(pt(scale))
                elif r < 0.65:
                    rec.curveTo(*[pt(scale) for _ in range(rng.choice([3, 3, 4]))])
                else:
                    rec.qCurveTo(*[pt(scale) for _ in range(rng.choice([2, 2, 3]))])
            if rng.random() < 0.9:
                rec.closePath()
            else:
                rec.endPath()
        outlines.append(rec.value)

    def run(make, get):
        pen = make()
        try:
            for op, args in outline:
                getattr(pen, op)(*args)
            return get(pen)
        except Exception:
            return None

    def fmt(values):
        if values is None:
            return "!E"
        return " ".join(repr(float(v)) for v in values)

    stat_fields = [
        "area", "meanX", "meanY", "varianceX", "varianceY",
        "stddevX", "stddevY", "covariance", "correlation", "slant",
    ]
    lines = []
    for outline in outlines:
        fields = [
            segment_ops(outline),
            fmt(run(lambda: MomentsPen(), lambda p: [p.area, p.momentX, p.momentY, p.momentXX, p.momentXY, p.momentYY])),
            fmt(run(lambda: StatisticsPen(), lambda p: [getattr(p, f) for f in stat_fields])),
            fmt(run(lambda: StatisticsControlPen(), lambda p: [getattr(p, f) for f in stat_fields])),
            fmt(run(lambda: AreaPen(), lambda p: [p.value])),
            fmt(run(lambda: PerimeterPen(None, 0.005), lambda p: [p.value])),
            fmt(run(lambda: PerimeterPen(None, 0.0001), lambda p: [p.value])),
        ]
        for make in (lambda: BoundsPen(None), lambda: ControlBoundsPen(None)):
            b = run(make, lambda p: p.bounds)
            fields.append("None" if b is None else fmt(b))
        lines.append("\t".join(fields))
    # Some values depend on the platform's math library: the Rust test compares
    # exactly on this platform only (fontoxide_misc::float_vectors).
    lines.insert(0, f"# platform: {sys.platform}")
    MEASURE_OUT.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")


def main():
    measure_vectors()
    lines = []
    for kind in ("cubic", "quadratic"):
        glyphs = GlyphSet(str(DATA / kind))
        for name in sorted(glyphs.keys()):
            rec = RecordingPen()
            glyphs[name].draw(rec)
            lines.append(f"{kind}\t{name}\tS\t{segment_ops(rec.value)}")
            prec = RecordingPointPen()
            glyphs[name].drawPoints(prec)
            lines.append(f"{kind}\t{name}\tP\t{point_ops(prec.value)}")
    for out in OUTS:
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
